use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;

use serde_json::{json, Value};

use crate::domain::openapi::{
    OpenApiBundleResult, OpenApiError, RefDiagnostic, SourceRef, SpecsRepoInfo,
};
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
        // Корневая запись: всё, что объявлено прямо во входном документе,
        // ищется по префиксу и находит именно её.
        sources: RefCell::new(vec![SourceRef {
            pointer: String::new(),
            file: entry_file_relative.to_string(),
            fragment: String::new(),
        }]),
        cycle_components: RefCell::new(HashMap::new()),
        taken_component_names: RefCell::new(
            root_value
                .pointer("/components/schemas")
                .and_then(|v| v.as_object())
                .map(|map| map.keys().cloned().collect())
                .unwrap_or_default(),
        ),
        enable_ref_fallback,
    };
    let mut document = resolver.walk(&root_value, &entry_path, "");
    resolver.inject_cycle_components(&mut document);
    Ok(OpenApiBundleResult {
        document,
        diagnostics: resolver.diagnostics.into_inner(),
        sources: resolver.sources.into_inner(),
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
    /// Куда в исходных файлах ведёт каждая успешно разрешённая граница
    /// `$ref` — см. [`SourceRef`].
    sources: RefCell<Vec<SourceRef>>,
    /// Цели, на которых замкнулась рекурсия: (файл, фрагмент) -> имя, под
    /// которым схема вынесена в `components/schemas` собранного документа.
    cycle_components: RefCell<HashMap<(PathBuf, String), String>>,
    /// Имена, уже занятые в `components/schemas` входного документа, — чтобы
    /// вынесенная схема не затёрла объявленную вручную.
    taken_component_names: RefCell<HashSet<String>>,
    /// Whether a missing well-known common-spec bundle should fall back to
    /// the app's bundled default copy instead of reporting "file not found".
    enable_ref_fallback: bool,
}

/// Имя компонента из цели ссылки: последний сегмент фрагмента
/// (`#/components/schemas/Node` -> `Node`), иначе — имя файла без расширения
/// (`schemas/node.yaml` -> `node`). Символы вне допустимого для ключа
/// `components/schemas` набора заменяются подчёркиванием.
fn component_base_name(file: &Path, fragment: &str) -> String {
    let raw = fragment
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
        .or_else(|| {
            file.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "Schema".to_string());
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if cleaned.is_empty() {
        "Schema".to_string()
    } else {
        cleaned
    }
}

impl Resolver {
    /// Имя, под которым рекурсивная цель живёт в `components/schemas`.
    /// Стабильно для одной и той же цели и не конфликтует ни с уже
    /// объявленными схемами, ни с другой вынесенной целью.
    fn component_name_for(&self, key: &(PathBuf, String)) -> String {
        if let Some(name) = self.cycle_components.borrow().get(key) {
            return name.clone();
        }
        let base = component_base_name(&key.0, &key.1);
        let mut candidate = base.clone();
        let mut suffix = 2;
        while self.taken_component_names.borrow().contains(&candidate) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        self.taken_component_names
            .borrow_mut()
            .insert(candidate.clone());
        self.cycle_components
            .borrow_mut()
            .insert(key.clone(), candidate.clone());
        candidate
    }

    /// Кладёт разрешённые тела рекурсивных схем в `components/schemas`
    /// собранного документа. Вызывается после обхода: к этому моменту внешнее
    /// раскрытие цели уже завершилось и лежит в `resolved_cache` — вместе с
    /// внутренней ссылкой на саму себя, которую мы там и оставили.
    fn inject_cycle_components(&self, document: &mut Value) {
        let pending: Vec<((PathBuf, String), String)> = self
            .cycle_components
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if pending.is_empty() {
            return;
        }
        for (key, name) in pending {
            // Цель может остаться неразрешённой — например, когда ссылка
            // ведёт на сам входной документ, который никогда не проходит
            // через `resolve_ref`. Тела для неё нет, но ссылку в никуда
            // оставлять нельзя: кладём заглушку, чтобы документ остался
            // структурно целым, и сообщаем об этом отдельной диагностикой.
            let resolved = self.resolved_cache.borrow().get(&key).cloned();
            let body = match resolved {
                Some(value) => (*value).clone(),
                None => {
                    self.diagnostics.borrow_mut().push(RefDiagnostic {
                        pointer: format!("/components/schemas/{name}"),
                        reference: format!("{}#{}", display_relative(&self.repo_root, &key.0), key.1),
                        referenced_from: display_relative(&self.repo_root, &key.0),
                        reason: "circular reference could not be materialized".to_string(),
                    });
                    json!({ "description": "рекурсивная ссылка: тело не раскрыто" })
                }
            };
            let components = document
                .as_object_mut()
                .expect("bundled document is an object")
                .entry("components")
                .or_insert_with(|| json!({}));
            let Some(components) = components.as_object_mut() else {
                continue;
            };
            let schemas = components
                .entry("schemas")
                .or_insert_with(|| json!({}));
            if let Some(schemas) = schemas.as_object_mut() {
                schemas.insert(name, body);
            }
        }
    }

    fn record_source(&self, doc_pointer: &str, target_file: &Path, fragment: &str) {
        self.sources.borrow_mut().push(SourceRef {
            pointer: doc_pointer.to_string(),
            file: display_relative(&self.repo_root, target_file),
            fragment: fragment.to_string(),
        });
    }

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
            // Рекурсивная схема (`Node.children: [Node]`, дерево, связный
            // список) — законная и очень частая конструкция OpenAPI, а не
            // ошибка спеки: генераторы кода разбирают её без вопросов.
            // Инлайнить её нельзя — раскрытие не завершится, — поэтому цель
            // выносим в `components/schemas` и оставляем здесь нормальную
            // внутреннюю ссылку. Так собранный документ остаётся валидным
            // OpenAPI: его можно отдать генератору или Swagger UI как есть.
            let name = self.component_name_for(&cache_key);
            return json!({ "$ref": format!("#/components/schemas/{name}") });
        }
        if let Some(cached) = self.resolved_cache.borrow().get(&cache_key) {
            // Кэш отдаёт готовое поддерево, но источник у этой позиции в
            // сборке свой — без записи здесь второе и последующие вхождения
            // одной схемы остались бы без исходника.
            self.record_source(doc_pointer, &target_file, frag);
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

        self.record_source(doc_pointer, &target_file, frag);
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
            sources: RefCell::new(Vec::new()),
            cycle_components: RefCell::new(HashMap::new()),
            taken_component_names: RefCell::new(HashSet::new()),
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
    fn records_the_source_file_of_every_resolved_ref() {
        let root = temp_dir();
        fs::create_dir_all(root.join("specs/operations")).unwrap();
        fs::create_dir_all(root.join("specs/schemas")).unwrap();
        fs::write(
            root.join("specs/api.yaml"),
            concat!(
                "openapi: 3.0.3\n",
                "info:\n  title: t\n  version: '1'\n",
                "paths:\n",
                "  /pets:\n",
                "    get:\n      $ref: './operations/listPets.yaml'\n",
                "    post:\n      $ref: './operations/createPet.yaml'\n",
            ),
        )
        .unwrap();
        fs::write(
            root.join("specs/operations/listPets.yaml"),
            "operationId: listPets\nresponses:\n  '200':\n    description: ok\n    content:\n      application/json:\n        schema:\n          $ref: '../schemas/all.yaml#/Pet'\n",
        )
        .unwrap();
        // Вторая операция ссылается на ту же схему — источник должен быть
        // записан и для неё, несмотря на кэш разрешённых поддеревьев.
        fs::write(
            root.join("specs/operations/createPet.yaml"),
            "operationId: createPet\nrequestBody:\n  content:\n    application/json:\n      schema:\n        $ref: '../schemas/all.yaml#/Pet'\n",
        )
        .unwrap();
        fs::write(
            root.join("specs/schemas/all.yaml"),
            "Pet:\n  type: object\n  properties:\n    name:\n      type: string\n",
        )
        .unwrap();

        let result = load_openapi_bundle(&root, "specs/api.yaml", false).unwrap();
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);

        let source_of = |pointer: &str| {
            result
                .sources
                .iter()
                .find(|s| s.pointer == pointer)
                .unwrap_or_else(|| panic!("no source recorded for {pointer}"))
        };

        assert_eq!(source_of("").file, "specs/api.yaml");
        assert_eq!(source_of("/paths/~1pets/get").file, "specs/operations/listPets.yaml");
        assert_eq!(source_of("/paths/~1pets/get").fragment, "");

        let schema = source_of("/paths/~1pets/get/responses/200/content/application~1json/schema");
        assert_eq!(schema.file, "specs/schemas/all.yaml");
        assert_eq!(schema.fragment, "/Pet");

        let reused =
            source_of("/paths/~1pets/post/requestBody/content/application~1json/schema");
        assert_eq!(reused.file, "specs/schemas/all.yaml");

        fs::remove_dir_all(&root).ok();
    }

    /// Спека с рекурсивной схемой: `Node.parent` и `Node.children[]` ведут на
    /// сам `Node`. Совершенно легальный OpenAPI — генераторы кода собирают из
    /// него рекурсивный тип, — поэтому диагностики быть не должно, а сборка
    /// обязана остаться валидным документом, годным для генератора.
    fn setup_recursive_repo() -> PathBuf {
        let root = temp_dir();
        fs::create_dir_all(root.join("specs/operations")).unwrap();
        fs::create_dir_all(root.join("specs/schemas")).unwrap();
        fs::write(
            root.join("specs/api.yaml"),
            "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths:\n  /nodes:\n    get:\n      $ref: './operations/listNodes.yaml'\n",
        )
        .unwrap();
        fs::write(
            root.join("specs/operations/listNodes.yaml"),
            "operationId: listNodes\nresponses:\n  '200':\n    description: ok\n    content:\n      application/json:\n        schema:\n          $ref: '../schemas/all.yaml#/Node'\n",
        )
        .unwrap();
        fs::write(
            root.join("specs/schemas/all.yaml"),
            concat!(
                "Node:\n",
                "  type: object\n",
                "  properties:\n",
                "    name:\n      type: string\n",
                "    parent:\n      $ref: '#/Node'\n",
                "    children:\n      type: array\n      items:\n        $ref: '#/Node'\n",
            ),
        )
        .unwrap();
        root
    }

    #[test]
    fn recursive_schema_is_hoisted_instead_of_being_reported_as_broken() {
        let root = setup_recursive_repo();
        let result = load_openapi_bundle(&root, "specs/api.yaml", false).unwrap();

        assert!(
            result.diagnostics.is_empty(),
            "рекурсия — не ошибка спеки, got {:?}",
            result.diagnostics
        );

        let schema = result
            .document
            .pointer("/paths/~1nodes/get/responses/200/content/application~1json/schema")
            .unwrap();
        // Внешняя схема развёрнута как обычно...
        assert_eq!(schema.pointer("/properties/name/type").unwrap(), "string");
        // ...а рекурсивные позиции стали нормальными внутренними ссылками.
        assert_eq!(
            schema.pointer("/properties/parent/$ref").unwrap(),
            "#/components/schemas/Node"
        );
        assert_eq!(
            schema.pointer("/properties/children/items/$ref").unwrap(),
            "#/components/schemas/Node"
        );
        // Ссылки ведут на реально существующий узел с телом схемы.
        let hoisted = result
            .document
            .pointer("/components/schemas/Node")
            .expect("рекурсивная схема вынесена в components/schemas");
        assert_eq!(hoisted.pointer("/properties/name/type").unwrap(), "string");
        assert_eq!(
            hoisted.pointer("/properties/parent/$ref").unwrap(),
            "#/components/schemas/Node"
        );
        // Нестандартных ключей вроде `circular` в документе не остаётся.
        assert!(!serde_json::to_string(&result.document)
            .unwrap()
            .contains("\"circular\""));

        fs::remove_dir_all(&root).ok();
    }

    /// Собирает все `$ref`, оставшиеся в документе после сборки.
    fn collect_refs(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(r)) = map.get("$ref") {
                    out.push(r.clone());
                }
                for v in map.values() {
                    collect_refs(v, out);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    collect_refs(v, out);
                }
            }
            _ => {}
        }
    }

    /// Главное требование к сборке: её можно отдать генератору кода или
    /// Swagger UI как один файл. Значит, каждая оставшаяся ссылка обязана
    /// разрешаться внутри самого документа — наружу не должно вести ничего.
    #[test]
    fn bundled_recursive_spec_is_self_contained() {
        let root = setup_recursive_repo();
        let result = load_openapi_bundle(&root, "specs/api.yaml", false).unwrap();

        let mut refs = Vec::new();
        collect_refs(&result.document, &mut refs);
        assert!(!refs.is_empty(), "рекурсия должна оставить ссылки");

        for reference in refs {
            assert!(
                reference.starts_with("#/"),
                "внешняя ссылка {reference} осталась в сборке"
            );
            assert!(
                resolve_pointer(&result.document, reference.trim_start_matches('#')).is_some(),
                "ссылка {reference} никуда не ведёт"
            );
        }

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn hoisted_schema_does_not_clobber_one_declared_in_the_entry_document() {
        let root = setup_recursive_repo();
        fs::write(
            root.join("specs/api.yaml"),
            concat!(
                "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\n",
                "components:\n  schemas:\n    Node:\n      type: string\n",
                "paths:\n  /nodes:\n    get:\n      $ref: './operations/listNodes.yaml'\n",
            ),
        )
        .unwrap();

        let result = load_openapi_bundle(&root, "specs/api.yaml", false).unwrap();
        assert_eq!(
            result.document.pointer("/components/schemas/Node/type").unwrap(),
            "string",
            "объявленная вручную схема должна уцелеть"
        );
        assert_eq!(
            result
                .document
                .pointer("/paths/~1nodes/get/responses/200/content/application~1json/schema/properties/parent/$ref")
                .unwrap(),
            "#/components/schemas/Node_2"
        );
        assert!(result
            .document
            .pointer("/components/schemas/Node_2/properties/name")
            .is_some());

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

