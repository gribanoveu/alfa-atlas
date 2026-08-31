use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;

use serde_json::{json, Value};

use crate::domain::openapi::{OpenApiBundleResult, OpenApiError, RefDiagnostic, SpecsRepoInfo};
use crate::domain::paths;
use crate::infra::common_spec_assets;

/// Conventional structural subfolders of this OpenAPI multi-file spec
/// convention. None are individually required — real-world spec repos
/// sometimes omit one (e.g. no `parameters/` if no operation needs extra
/// parameters) — each is instead an independent scored signal, see
/// [`score_specs_signals`].
pub const KNOWN_SUBDIRS: [&str; 4] = ["schemas", "responses", "parameters", "operations"];

const ENTRY_FILE_POINTS: u32 = 40;
const SUBDIR_POINTS: u32 = 15;

/// Minimum [`score_specs_signals`] score for [`specs_root_signature`] to
/// treat a directory as a spec root worth bundling for the API Explorer:
/// the entry document plus at least one structural subfolder. The entry
/// document itself is effectively still mandatory — without one there is
/// nothing to bundle, and `score_specs_signals` never returns an `entry`
/// without having found it, regardless of how the subfolder points add up.
const DETECTION_THRESHOLD: u32 = ENTRY_FILE_POINTS + SUBDIR_POINTS;

const SPEC_EXTS: [&str; 3] = ["yaml", "yml", "json"];

/// The entry document found directly inside a detected specs root, plus
/// whatever `info.title`/`info.version` it declares.
pub struct SpecsRootSignature {
    pub entry_path: PathBuf,
    pub title: Option<String>,
    pub version: Option<String>,
}

/// How strongly a directory looks like an OpenAPI multi-file spec root,
/// as an additive score rather than a pass/fail gate on a fixed required
/// set. Used by the docs-root discovery heuristic to weigh "this is a spec
/// root" against its own "this is a docs root" score for the very same
/// directory, and let whichever is higher win.
pub struct SpecsSignal {
    pub score: u32,
    pub entry: Option<SpecsRootSignature>,
}

/// Scores `dir` on how strongly it looks like an OpenAPI multi-file spec
/// root: [`ENTRY_FILE_POINTS`] for a YAML/JSON file directly inside `dir`
/// (not recursing into subfolders) with a top-level `openapi:`/`swagger:`
/// key, plus [`SUBDIR_POINTS`] for each of [`KNOWN_SUBDIRS`] actually
/// present. No single signal is required for a nonzero score.
pub fn score_specs_signals(dir: &Path) -> SpecsSignal {
    if !dir.is_dir() {
        return SpecsSignal {
            score: 0,
            entry: None,
        };
    }

    let entry = find_entry_file(dir);
    let mut score = if entry.is_some() { ENTRY_FILE_POINTS } else { 0 };
    for sub in KNOWN_SUBDIRS {
        if dir.join(sub).is_dir() {
            score += SUBDIR_POINTS;
        }
    }
    SpecsSignal { score, entry }
}

/// Scans the files directly inside `dir` (not recursing into subfolders)
/// for one with a top-level `openapi:`/`swagger:` key.
fn find_entry_file(dir: &Path) -> Option<SpecsRootSignature> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| {
            ext_of(p)
                .map(|ext| SPEC_EXTS.contains(&ext.as_str()))
                .unwrap_or(false)
        })
        .collect();
    entries.sort();

    for path in entries {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(ext) = ext_of(&path) else { continue };
        let Ok(value) = parse_generic(&text, &ext) else {
            continue;
        };
        let Some(obj) = value.as_object() else {
            continue;
        };
        if obj.contains_key("openapi") || obj.contains_key("swagger") {
            let title = value
                .pointer("/info/title")
                .and_then(|v| v.as_str())
                .map(String::from);
            let version = value
                .pointer("/info/version")
                .and_then(|v| v.as_str())
                .map(String::from);
            return Some(SpecsRootSignature {
                entry_path: path,
                title,
                version,
            });
        }
    }

    None
}

/// Checks whether `dir` scores highly enough ([`score_specs_signals`] >=
/// [`DETECTION_THRESHOLD`]) to be treated as a spec root worth bundling.
/// Shared conceptually with the docs-root discovery heuristic (both are
/// built on `score_specs_signals`), but this one applies a fixed threshold
/// rather than comparing against a competing docs score, since it's used to
/// gate a specific feature (the API Explorer) rather than to rank candidates.
pub fn specs_root_signature(dir: &Path) -> Option<SpecsRootSignature> {
    let signal = score_specs_signals(dir);
    if signal.score < DETECTION_THRESHOLD {
        return None;
    }
    signal.entry
}

/// Detects whether `repo_root/specs` follows this OpenAPI multi-file spec
/// convention closely enough ([`specs_root_signature`]) to bundle, and if
/// so, finds the entry document. This gate is independent of the docs-root
/// discovery heuristic (`docs_discovery.rs`), which separately weighs the
/// same underlying score against a competing "looks like documentation"
/// score to decide what to suggest as a project's docs root.
pub fn detect_specs_repo(repo_root: &Path) -> Result<Option<SpecsRepoInfo>, OpenApiError> {
    let specs_root = repo_root.join("specs");
    let Some(sig) = specs_root_signature(&specs_root) else {
        return Ok(None);
    };

    let entry_file = paths::relative_to(repo_root, &sig.entry_path)?;
    let specs_root_canonical = specs_root
        .canonicalize()
        .map_err(crate::domain::project_config::ProjectError::Canonicalize)?;
    Ok(Some(SpecsRepoInfo {
        specs_root: specs_root_canonical.to_string_lossy().into_owned(),
        entry_file,
        title: sig.title,
        version: sig.version,
    }))
}

/// Reads the entry document and every file it (transitively) references via
/// `$ref`, fully inlining them into a single document. Only the entry file is
/// boundary-checked against `repo_root` (`paths::ensure_under`) — followed
/// `$ref`s are resolved lexically and are allowed to reach outside `specs/`
/// or even the repository (a common pattern: shared/common-spec artifacts
/// pulled in at build time), since this is read-only viewing. A `$ref` that
/// can't be resolved (missing file, missing pointer, or a cycle) is recorded
/// as a `RefDiagnostic` and replaced with an inline marker instead of failing
/// the whole load.
pub fn load_openapi_bundle(
    repo_root: &Path,
    entry_file_relative: &str,
    enable_ref_fallback: bool,
) -> Result<OpenApiBundleResult, OpenApiError> {
    let joined = paths::join_relative(repo_root, entry_file_relative)?;
    let entry_path = paths::ensure_under(repo_root, &joined)?;

    let text = fs::read_to_string(&entry_path)
        .map_err(|e| OpenApiError::Read(entry_file_relative.to_string(), e))?;
    let ext = ext_of(&entry_path).unwrap_or_default();
    let root_value = parse_generic(&text, &ext)
        .map_err(|e| OpenApiError::Parse(entry_file_relative.to_string(), e))?;
    if root_value.get("openapi").is_none() && root_value.get("swagger").is_none() {
        return Err(OpenApiError::NotOpenApi(entry_file_relative.to_string()));
    }

    let resolver = Resolver {
        repo_root: repo_root.to_path_buf(),
        file_cache: RefCell::new(HashMap::new()),
        resolved_cache: RefCell::new(HashMap::new()),
        in_progress: RefCell::new(HashSet::new()),
        diagnostics: RefCell::new(Vec::new()),
        enable_ref_fallback,
    };
    let document = resolver.walk(&root_value, &entry_path, "");
    Ok(OpenApiBundleResult {
        document,
        diagnostics: resolver.diagnostics.into_inner(),
    })
}

fn ext_of(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
}

/// Parses YAML/JSON text into a generic `serde_json::Value`. YAML goes
/// through `serde_yaml::Value` first, then converts to `serde_json::Value` —
/// OpenAPI documents commonly use integer map keys for HTTP status codes
/// (`200:`, `404:`), which `serde_yaml` deserializes fine into its own
/// `Value` but which `serde_json::Value` (string-keyed maps only) can't
/// accept directly from `serde_yaml::from_str`. The two-step conversion
/// stringifies those keys correctly instead of erroring.
fn parse_generic(text: &str, ext: &str) -> Result<Value, String> {
    if ext == "json" {
        serde_json::from_str(text).map_err(|e| e.to_string())
    } else {
        let yaml_value: serde_yaml::Value = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
        serde_json::to_value(yaml_value).map_err(|e| e.to_string())
    }
}

/// RFC 6901 JSON Pointer resolution against a `serde_json::Value`.
/// An empty pointer resolves to the whole document (used for whole-file refs).
fn resolve_pointer<'a>(root: &'a Value, pointer: &str) -> Option<&'a Value> {
    if pointer.is_empty() {
        return Some(root);
    }
    let mut current = root;
    for raw in pointer.trim_start_matches('/').split('/') {
        let seg = raw.replace("~1", "/").replace("~0", "~");
        current = match current {
            Value::Object(map) => map.get(&seg)?,
            Value::Array(arr) => arr.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

/// Splits a `$ref` string into its file part and fragment (without the `#`).
/// `"./schemas/all.yaml#/Foo"` -> `("./schemas/all.yaml", "/Foo")`.
/// `"#/taxId"` -> `("", "/taxId")`. `"./operations/foo.yaml"` -> `(".../foo.yaml", "")`.
fn split_ref(r: &str) -> (&str, &str) {
    match r.split_once('#') {
        Some((file, frag)) => (file, frag),
        None => (r, ""),
    }
}

/// Joins `base_dir` with a relative ref path, resolving `.`/`..` purely
/// lexically (no filesystem access, no `canonicalize`) — the target may
/// legitimately not exist on disk (e.g. a build artifact that hasn't been
/// generated yet), and `canonicalize` would hard-error on that.
fn normalize_join(base_dir: &Path, rel: &str) -> PathBuf {
    let mut stack: Vec<Component> = base_dir.components().collect();
    for part in Path::new(rel).components() {
        match part {
            Component::ParentDir => {
                stack.pop();
            }
            Component::CurDir => {}
            other => stack.push(other),
        }
    }
    stack.into_iter().collect()
}

/// Real-world spec repos mix `.yaml`/`.yml` extensions for otherwise
/// equivalent files; tolerate a whole-file ref written with the "wrong" one.
fn ext_fallback_candidates(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_path_buf()];
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let swapped = match ext.to_ascii_lowercase().as_str() {
            "yaml" => Some("yml"),
            "yml" => Some("yaml"),
            _ => None,
        };
        if let Some(alt) = swapped {
            out.push(path.with_extension(alt));
        }
    }
    out
}

fn display_relative(repo_root: &Path, p: &Path) -> String {
    paths::relative_to(repo_root, p).unwrap_or_else(|_| p.display().to_string())
}

/// Matches the well-known location of the "common" spec bundle that
/// `ru.alfalab.openapi-configurer`-style Gradle projects extract from a
/// published jar into `build/common/META-INF/specs/api.yaml` at build time.
/// That path is always gitignored and only exists once a Java/Gradle build
/// has run, so a repo opened here without that build step is missing it —
/// matched by suffix, independent of the `build/` prefix (which is just
/// where the referring `$ref` happened to resolve it lexically).
///
/// Shared with `services::diagnostics`, which must stay in sync with the
/// resolver: a `$ref` the resolver satisfies from the bundled copy must not
/// also be reported as a broken include in the Problems panel.
pub(crate) fn is_common_spec_fallback_path(path: &Path) -> bool {
    let mut tail: Vec<&str> = path
        .components()
        .rev()
        .take(4)
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    tail.reverse();
    tail.as_slice() == ["common", "META-INF", "specs", "api.yaml"]
}

/// Parses the bundled default copy of the common spec bundle once and caches
/// the result for the process lifetime. Cached as a plain `Value` (`Rc` isn't
/// `Sync`, so it can't live in a `static`); each call wraps a clone in a
/// fresh `Rc` for the (single-threaded) resolver to cache further itself.
fn bundled_common_spec_value() -> Rc<Value> {
    static CACHE: OnceLock<Value> = OnceLock::new();
    let value = CACHE.get_or_init(|| {
        let text = common_spec_assets::bundled_common_api_yaml();
        parse_generic(text, "yaml").expect("bundled common-spec asset must be valid YAML")
    });
    Rc::new(value.clone())
}

struct Resolver {
    repo_root: PathBuf,
    /// Raw parsed file content, keyed by lexically-normalized path.
    file_cache: RefCell<HashMap<PathBuf, Rc<Value>>>,
    /// Fully-inlined result per (file, pointer), so a fragment referenced
    /// from multiple places is only walked once.
    resolved_cache: RefCell<HashMap<(PathBuf, String), Rc<Value>>>,
    /// (file, pointer) pairs currently being resolved, for cycle detection.
    in_progress: RefCell<HashSet<(PathBuf, String)>>,
    diagnostics: RefCell<Vec<RefDiagnostic>>,
    /// Whether a missing well-known common-spec bundle should fall back to
    /// the app's bundled default copy instead of reporting "file not found".
    enable_ref_fallback: bool,
}

impl Resolver {
    fn load_file(&self, path: &Path) -> Result<Rc<Value>, ()> {
        if let Some(v) = self.file_cache.borrow().get(path) {
            return Ok(v.clone());
        }
        for candidate in ext_fallback_candidates(path) {
            let Ok(text) = fs::read_to_string(&candidate) else {
                continue;
            };
            let Some(ext) = ext_of(&candidate) else {
                continue;
            };
            if let Ok(value) = parse_generic(&text, &ext) {
                let rc = Rc::new(value);
                self.file_cache.borrow_mut().insert(path.to_path_buf(), rc.clone());
                return Ok(rc);
            }
        }
        if self.enable_ref_fallback && is_common_spec_fallback_path(path) {
            let rc = bundled_common_spec_value();
            self.file_cache.borrow_mut().insert(path.to_path_buf(), rc.clone());
            return Ok(rc);
        }
        Err(())
    }

    /// Resolves one `$ref` string found in `current_file`, returning its
    /// fully-inlined replacement. `doc_pointer` is this node's location in
    /// the *output* document, used only for diagnostics.
    fn resolve_ref(&self, ref_str: &str, current_file: &Path, doc_pointer: &str) -> Value {
        let (file_part, frag) = split_ref(ref_str);
        let target_file = if file_part.is_empty() {
            current_file.to_path_buf()
        } else {
            normalize_join(
                current_file.parent().unwrap_or(current_file),
                file_part,
            )
        };
        let cache_key = (target_file.clone(), frag.to_string());

        if self.in_progress.borrow().contains(&cache_key) {
            self.diagnostics.borrow_mut().push(RefDiagnostic {
                pointer: doc_pointer.to_string(),
                reference: ref_str.to_string(),
                referenced_from: display_relative(&self.repo_root, current_file),
                reason: "circular reference".to_string(),
            });
            return json!({ "$ref": ref_str, "circular": true });
        }
        if let Some(cached) = self.resolved_cache.borrow().get(&cache_key) {
            return (**cached).clone();
        }

        let Ok(file_value) = self.load_file(&target_file) else {
            self.diagnostics.borrow_mut().push(RefDiagnostic {
                pointer: doc_pointer.to_string(),
                reference: ref_str.to_string(),
                referenced_from: display_relative(&self.repo_root, current_file),
                reason: "file not found".to_string(),
            });
            return json!({ "$ref": ref_str, "unresolved": true, "reason": "file not found" });
        };

        let Some(target_node) = resolve_pointer(&file_value, frag) else {
            self.diagnostics.borrow_mut().push(RefDiagnostic {
                pointer: doc_pointer.to_string(),
                reference: ref_str.to_string(),
                referenced_from: display_relative(&self.repo_root, current_file),
                reason: "pointer not found".to_string(),
            });
            return json!({ "$ref": ref_str, "unresolved": true, "reason": "pointer not found" });
        };
        let target_node = target_node.clone();

        self.in_progress.borrow_mut().insert(cache_key.clone());
        // Further relative $refs inside the target resolve relative to
        // where the target file lives, not the original referrer.
        let resolved = self.walk(&target_node, &target_file, doc_pointer);
        self.in_progress.borrow_mut().remove(&cache_key);

        self.resolved_cache
            .borrow_mut()
            .insert(cache_key, Rc::new(resolved.clone()));
        resolved
    }

    fn walk(&self, value: &Value, current_file: &Path, doc_pointer: &str) -> Value {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(r)) = map.get("$ref") {
                    let mut resolved = self.resolve_ref(r, current_file, doc_pointer);
                    // Sibling keys next to $ref are non-standard but tolerated
                    // as local overrides on top of the resolved value.
                    if map.len() > 1 {
                        if let Value::Object(resolved_map) = &mut resolved {
                            for (k, v) in map {
                                if k != "$ref" {
                                    resolved_map.insert(k.clone(), v.clone());
                                }
                            }
                        }
                    }
                    return resolved;
                }
                let mut out = serde_json::Map::new();
                for (k, v) in map {
                    let child_ptr =
                        format!("{doc_pointer}/{}", k.replace('~', "~0").replace('/', "~1"));
                    out.insert(k.clone(), self.walk(v, current_file, &child_ptr));
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(
                arr.iter()
                    .enumerate()
                    .map(|(i, v)| self.walk(v, current_file, &format!("{doc_pointer}/{i}")))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("alfa-atlas-openapi-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Sets up a minimal spec repo whose `specs/responses/all.yaml` refs the
    /// well-known common-spec bundle path, without that build artifact
    /// actually existing on disk — mirroring a spec repo opened without a
    /// prior Gradle build.
    fn setup_repo_with_missing_common_spec() -> PathBuf {
        let root = temp_dir();
        fs::create_dir_all(root.join("specs/responses")).unwrap();
        fs::write(
            root.join("specs/api.yaml"),
            "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\n",
        )
        .unwrap();
        fs::write(
            root.join("specs/responses/all.yaml"),
            "badRequest:\n  $ref: '../../build/common/META-INF/specs/api.yaml#/components/responses/badRequest'\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn falls_back_to_bundled_common_spec_when_enabled() {
        let root = setup_repo_with_missing_common_spec();
        let result = load_openapi_bundle(&root, "specs/api.yaml", true).unwrap();
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            result.diagnostics
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reports_file_not_found_when_fallback_disabled() {
        let root = setup_repo_with_missing_common_spec();
        // The $ref lives in specs/responses/all.yaml, which the minimal
        // entry document doesn't itself reference; walk it directly to
        // exercise the resolver's file-not-found path.
        let resolver = Resolver {
            repo_root: root.clone(),
            file_cache: RefCell::new(HashMap::new()),
            resolved_cache: RefCell::new(HashMap::new()),
            in_progress: RefCell::new(HashSet::new()),
            diagnostics: RefCell::new(Vec::new()),
            enable_ref_fallback: false,
        };
        let text = fs::read_to_string(root.join("specs/responses/all.yaml")).unwrap();
        let value = parse_generic(&text, "yaml").unwrap();
        resolver.walk(&value, &root.join("specs/responses/all.yaml"), "");
        let diagnostics = resolver.diagnostics.into_inner();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].reason, "file not found");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn is_common_spec_fallback_path_matches_suffix_regardless_of_prefix() {
        assert!(is_common_spec_fallback_path(Path::new(
            "/repo/build/common/META-INF/specs/api.yaml"
        )));
        assert!(!is_common_spec_fallback_path(Path::new(
            "/repo/build/common/META-INF/specs/other.yaml"
        )));
        assert!(!is_common_spec_fallback_path(Path::new(
            "/repo/specs/schemas/api.yaml"
        )));
    }
}
