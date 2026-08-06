//! Executor for the read-only tools a future AI harness will call. This is
//! the enforcement point for `AiAccessMode`: every function here resolves
//! containment against `scope.root` via `domain::paths` — the same
//! primitives `services::docs_fs` uses — so a caller can never widen access
//! by passing an unexpected path, only by the `ToolScope` itself having been
//! constructed with the wider root.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, TryLockError};

use crate::commands::embeddings::{
    attach_embedding_index, attach_index_store, ensure_provider, resolve_index_paths,
    EmbeddingIndexSlot, EmbeddingProviderSlot, EmbeddingSyncGuard, IndexStoreSlot,
};
use crate::domain::ai_access::{default_allowed_tools, AiAccessMode, ToolName};
use crate::domain::ai_tools::{
    ListFilesArgs, MatchSource, ReadFileArgs, SemanticSearchArgs, ToolCall, ToolError,
    ToolFileEntry, ToolMatch, ToolResult, ToolScope,
};
use crate::domain::chunk_index::{qualified_name_for, ChunkMetadata};
use crate::domain::paths;
use crate::domain::project_config::{ProjectConfig, ProjectError, TreeNode};
use crate::domain::repo_index::{FileId, Symbol};
use crate::infra::{embedding_credentials_store, embedding_providers, project_store, workspace_scanner};
use crate::services::chunk_builder::ChunkIndex;
use crate::services::chunk_text::resolve_text;
use crate::services::repo_index::RepositoryIndex;
use crate::services::{docs_fs, embedding_config, project_open};

const DEFAULT_TOP_K: usize = 10;
const MAX_TOP_K: usize = 50;
/// Cap on how many characters of matched text land in a `ToolMatch.snippet`
/// — keeps a large chunk's (up to 16KB) full text from blowing up the
/// response payload.
const SNIPPET_MAX_CHARS: usize = 500;

/// The embedding/chunk/repo-index state `SemanticSearch` needs to reach —
/// `execute_tool` is otherwise a pure function with no access to
/// Tauri-managed state. Mirrors exactly what
/// `commands::embeddings::embedding_sync` already receives as
/// `State<'_, Arc<T>>` params; `commands::ai_tools::ai_execute_tool` clones
/// each into this struct before calling `execute_tool`.
pub struct EmbeddingDeps {
    pub repo_index: Arc<RepositoryIndex>,
    pub chunk_index: Arc<ChunkIndex>,
    pub embedding_index: Arc<EmbeddingIndexSlot>,
    pub index_store: Arc<IndexStoreSlot>,
    pub embedding_provider: Arc<EmbeddingProviderSlot>,
    pub sync_guard: Arc<EmbeddingSyncGuard>,
}

#[cfg(test)]
impl EmbeddingDeps {
    /// Fresh, empty instances of every slot — for `ReadFile`/`ListFiles`
    /// tests (which never touch these) and as a base for `SemanticSearch`
    /// tests that need to populate specific state.
    pub fn empty() -> Self {
        Self {
            repo_index: Arc::new(RepositoryIndex::new()),
            chunk_index: Arc::new(ChunkIndex::new()),
            embedding_index: Arc::new(EmbeddingIndexSlot::new(None)),
            index_store: Arc::new(IndexStoreSlot::new(None)),
            embedding_provider: Arc::new(EmbeddingProviderSlot::new(None)),
            sync_guard: Arc::new(EmbeddingSyncGuard::new(())),
        }
    }
}

/// Single entry point for the harness: one allowlist check (via
/// `scope.allows`), one place to serialize a call/result at the LLM
/// boundary (`ToolCall`/`ToolResult` both derive `serde`), and — later —
/// one place to log every tool invocation (not wired up yet).
pub fn execute_tool(
    scope: &ToolScope,
    call: ToolCall,
    deps: &EmbeddingDeps,
) -> Result<ToolResult, ToolError> {
    if !scope.allows(call.name()) {
        return Err(ToolError::NotAllowed(call.name()));
    }
    match call {
        ToolCall::ReadFile(args) => read_file(scope, args).map(ToolResult::File),
        ToolCall::ListFiles(args) => list_files(scope, args).map(ToolResult::FileList),
        ToolCall::SemanticSearch(args) => {
            semantic_search(scope, args, deps).map(ToolResult::SemanticSearchResults)
        }
    }
}

/// Resolves a `ToolScope` from a project's persisted config — the one place
/// that turns "user hasn't customized anything" into `mode`'s default
/// allowlist, and a customized list into the authoritative one.
pub fn scope_for_config(repo_root: &Path, docs_root: &Path, config: &ProjectConfig) -> ToolScope {
    let allowed: HashSet<ToolName> = config
        .ai_allowed_tools
        .clone()
        .map(|v| v.into_iter().collect())
        .unwrap_or_else(|| default_allowed_tools(config.ai_access_mode));
    ToolScope::new(repo_root, docs_root, config.ai_access_mode, allowed)
}

/// Resolves a `ToolScope` for whichever project is currently open, without
/// the caller (the IPC command) supplying any path — this is what lets the
/// frontend call `ai_execute_tool` knowing nothing about `docsRoot`/
/// `repoRoot`/the access mode. Reuses the same backend-authoritative source
/// `commands::project::get_project` already uses at startup restore;
/// `project_open::get_project()` alone doesn't expose `ai_access_mode`/
/// `ai_allowed_tools` (it discards the rest of `ProjectConfig`), so those
/// are loaded separately here.
pub fn current_scope() -> Result<ToolScope, ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    Ok(scope_for_config(
        Path::new(&opened.root),
        Path::new(&opened.docs_root),
        &config,
    ))
}

fn list_files(scope: &ToolScope, args: ListFilesArgs) -> Result<Vec<ToolFileEntry>, ToolError> {
    let subdir = resolve_subdir(scope, args.path.as_deref())?;

    match scope.mode {
        AiAccessMode::DocsOnly => {
            list_docs_only(scope, subdir.as_ref().map(|(rel, _)| rel.as_str()))
        }
        AiAccessMode::FullRepo => list_full_repo(scope, subdir.map(|(_, abs)| abs)),
    }
}

fn read_file(scope: &ToolScope, args: ReadFileArgs) -> Result<String, ToolError> {
    // No extension filtering here, unlike `docs_fs::read_project_file` —
    // the tool boundary is containment under `scope.root` alone. In
    // `FullRepo` mode the harness must be able to read source files, which
    // aren't in `is_supported_file`'s doc-format list.
    let joined = paths::join_relative(&scope.root, &args.path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    if !canonical.exists() {
        return Err(ToolError::NotFound(args.path));
    }
    if !canonical.is_file() {
        return Err(ToolError::NotAFile(args.path));
    }
    fs::read_to_string(&canonical).map_err(ToolError::Io)
}

/// Validates an optional subdirectory argument once, shared by both mode
/// branches: returns its root-relative string form (for the docs-only
/// prefix filter) and its canonical absolute form (for the full-repo scan
/// root).
fn resolve_subdir(
    scope: &ToolScope,
    path: Option<&str>,
) -> Result<Option<(String, PathBuf)>, ToolError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if path.is_empty() || path == "." {
        return Ok(None);
    }
    let joined = paths::join_relative(&scope.root, path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    if !canonical.is_dir() {
        return Err(ToolError::NotFound(path.to_string()));
    }
    let rel = paths::relative_to(&scope.root, &canonical)?;
    Ok(Some((rel, canonical)))
}

fn list_docs_only(
    scope: &ToolScope,
    subdir_rel: Option<&str>,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    let tree = docs_fs::list_docs_tree(&scope.root.to_string_lossy())?;
    let mut entries = Vec::new();
    flatten_tree(tree, &mut entries);

    let Some(prefix) = subdir_rel else {
        return Ok(entries);
    };
    let with_slash = format!("{prefix}/");
    entries.retain(|e| e.path == prefix || e.path.starts_with(&with_slash));
    Ok(entries)
}

fn flatten_tree(nodes: Vec<TreeNode>, out: &mut Vec<ToolFileEntry>) {
    for node in nodes {
        out.push(ToolFileEntry {
            path: node.path,
            is_dir: node.is_dir,
        });
        if let Some(children) = node.children {
            flatten_tree(children, out);
        }
    }
}

fn list_full_repo(
    scope: &ToolScope,
    scan_root: Option<PathBuf>,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    let scan_root = scan_root.unwrap_or_else(|| scope.root.clone());
    let files = workspace_scanner::scan_all(&scan_root)?;
    files
        .into_iter()
        .map(|f| {
            let rel = paths::relative_to(&scope.root, &f.path)?;
            Ok(ToolFileEntry {
                path: rel,
                is_dir: false,
            })
        })
        .collect()
}

/// Cascade entry point: an exact symbol-name hit (cheapest, always tried)
/// is prepended to whichever of the semantic/lexical tiers fills the
/// remaining `top_k` budget, chosen by `is_semantic_ready`.
fn semantic_search(
    scope: &ToolScope,
    args: SemanticSearchArgs,
    deps: &EmbeddingDeps,
) -> Result<Vec<ToolMatch>, ToolError> {
    let top_k = args.top_k.unwrap_or(DEFAULT_TOP_K).clamp(1, MAX_TOP_K);

    let mut results = symbol_matches(&deps.repo_index, scope, &args.query, top_k);

    let remaining = top_k.saturating_sub(results.len());
    if remaining == 0 {
        return Ok(results);
    }

    if is_semantic_ready(deps) {
        results.extend(semantic_matches(scope, deps, &args.query, remaining)?);
    } else {
        results.extend(lexical_matches(&deps.chunk_index, scope, &args.query, remaining));
    }
    Ok(results)
}

/// Mirrors `commands::embeddings::embedding_index_status`'s readiness check
/// exactly (`resolve_index_paths` -> `attach_index_store` -> stale check ->
/// `attach_embedding_index(allow_repair: false)` -> `embedded_count > 0`),
/// plus a `try_lock` peek at `EmbeddingSyncGuard` for "a sync is actively
/// running right now". The guard is never held through this check or the
/// search that follows — its `try_lock` guard value is dropped immediately
/// (never bound to a variable), matching `embedding_index_status`'s own
/// precedent of never acquiring this guard at all for a read. Any failure
/// along the rest of this sequence (no project open, a transient
/// store-open error) degrades to "not ready" rather than propagating —
/// consistent with the whole feature being a graceful cascade, not a
/// pipeline that should hard-fail just because the fast path had a hiccup.
fn is_semantic_ready(deps: &EmbeddingDeps) -> bool {
    // `WouldBlock` (a sync is actively running right now) is the only
    // `try_lock` outcome that should degrade this call — `Poisoned` must
    // not, or a single panic elsewhere while holding this guard (see
    // `commands::embeddings::lock_sync_guard`'s doc comment) would disable
    // semantic search for the rest of the app's lifetime instead of just
    // this one call.
    if matches!(deps.sync_guard.try_lock(), Err(TryLockError::WouldBlock)) {
        return false;
    }

    let Ok(Some(project)) = project_open::get_project() else {
        return false;
    };
    let Ok((index_root, storage_dir)) = resolve_index_paths(&project) else {
        return false;
    };
    let Ok((store, stale)) =
        attach_index_store(&deps.chunk_index, &deps.index_store, &storage_dir, &index_root)
    else {
        return false;
    };
    if stale {
        return false;
    }

    let Ok(config) = embedding_config::load_embedding_config() else {
        return false;
    };
    let dimensions = embedding_providers::expected_dimensions(&config);
    if attach_embedding_index(&deps.embedding_index, &store, &index_root, dimensions, false).is_err()
    {
        return false;
    }

    let Ok(slot) = deps.embedding_index.lock() else {
        return false;
    };
    slot.as_ref().is_some_and(|(_, _, index)| index.len() > 0)
}

/// Embeds `query`, searches the resident `EmbeddingIndex`, and resolves
/// each hit's chunk text. Independently re-derives `index_root`/attaches
/// the store/index rather than reusing `is_semantic_ready`'s work — cheap
/// and idempotent (each attach short-circuits when already current),
/// matching how `embedding_sync`/`embedding_index_status` each separately
/// re-resolve this instead of sharing state across calls.
fn semantic_matches(
    scope: &ToolScope,
    deps: &EmbeddingDeps,
    query: &str,
    top_k: usize,
) -> Result<Vec<ToolMatch>, ToolError> {
    let project = project_open::get_project()
        .map_err(|e| ToolError::SemanticSearch(e.to_string()))?
        .ok_or_else(|| ToolError::SemanticSearch("no project is open".to_string()))?;
    let (index_root, storage_dir) =
        resolve_index_paths(&project).map_err(ToolError::SemanticSearch)?;
    let (store, _stale) = attach_index_store(
        &deps.chunk_index,
        &deps.index_store,
        &storage_dir,
        &index_root,
    )
    .map_err(ToolError::SemanticSearch)?;

    let config = embedding_config::load_embedding_config()
        .map_err(|e| ToolError::SemanticSearch(e.to_string()))?;
    let dimensions = embedding_providers::expected_dimensions(&config);
    attach_embedding_index(&deps.embedding_index, &store, &index_root, dimensions, false)
        .map_err(ToolError::SemanticSearch)?;

    let api_key = embedding_credentials_store::get_api_key();
    let provider = ensure_provider(&deps.embedding_provider, &config, api_key)
        .map_err(ToolError::SemanticSearch)?;

    let query_embedding = provider
        .embed(&[query])?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ToolError::SemanticSearch("embedding provider returned no vector".to_string())
        })?;

    let hits = {
        let slot = deps.embedding_index.lock().map_err(|_| {
            ToolError::SemanticSearch("embedding index lock poisoned".to_string())
        })?;
        let Some((_, _, index)) = slot.as_ref() else {
            return Ok(Vec::new());
        };
        // This `usearch` wrapper has no predicate-aware ANN search — when
        // this scope filters results (`DocsOnly`), over-fetch the whole
        // corpus so filtering below can still fill `top_k` from whatever's
        // left, rather than silently returning fewer hits than the caller
        // asked for just because the nearest raw neighbors happened to be
        // outside `docs_root`.
        let search_k = if scope.mode == AiAccessMode::DocsOnly {
            top_k.max(index.len())
        } else {
            top_k
        };
        index.search(&query_embedding, search_k)?
    };

    let mut out = Vec::with_capacity(top_k.min(hits.len()));
    for (chunk_id, distance) in hits {
        if out.len() >= top_k {
            break;
        }
        let Some(metadata) = deps.chunk_index.get(&chunk_id) else {
            continue;
        };
        if !scope.allows_search_result(&metadata.file_id) {
            continue;
        }
        let Ok(text) = resolve_text(&scope.repo_root, &metadata) else {
            continue;
        };
        out.push(ToolMatch {
            path: metadata.file_id.0,
            snippet: truncate_snippet(&text),
            // `EmbeddingIndex::search` returns cosine distance (lower is
            // closer) — flip to a "higher is better" similarity score.
            score: 1.0 - distance,
            start_byte: metadata.start_byte,
            end_byte: metadata.end_byte,
            qualified_name: metadata.qualified_name,
            source: MatchSource::Semantic,
        });
    }
    Ok(out)
}

/// No-embeddings fallback: scans every chunk's resolved text for a
/// case-insensitive substring match, ranked by occurrence count (a weak
/// proxy score — not comparable to the semantic tier's cosine similarity).
fn lexical_matches(
    chunk_index: &ChunkIndex,
    scope: &ToolScope,
    query: &str,
    top_k: usize,
) -> Vec<ToolMatch> {
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(usize, ChunkMetadata, String)> = Vec::new();
    for metadata in chunk_index.all() {
        if !scope.allows_search_result(&metadata.file_id) {
            continue;
        }
        let Ok(text) = resolve_text(&scope.repo_root, &metadata) else {
            continue;
        };
        let count = text.to_lowercase().matches(&needle).count();
        if count > 0 {
            scored.push((count, metadata, text));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(count, metadata, text)| ToolMatch {
            path: metadata.file_id.0,
            snippet: truncate_snippet(&text),
            score: count as f32,
            start_byte: metadata.start_byte,
            end_byte: metadata.end_byte,
            qualified_name: metadata.qualified_name,
            source: MatchSource::Lexical,
        })
        .collect()
}

/// Cheapest tier: an exact (case-insensitive) symbol-name match, no disk
/// I/O beyond a best-effort snippet read. Always tried first, regardless
/// of whether the embedding index is ready.
fn symbol_matches(
    repo_index: &RepositoryIndex,
    scope: &ToolScope,
    query: &str,
    top_k: usize,
) -> Vec<ToolMatch> {
    repo_index
        .find_symbol(query)
        .into_iter()
        .filter(|(file_id, _)| scope.allows_search_result(file_id))
        .take(top_k)
        .filter_map(|(file_id, symbol)| {
            let all_symbols = repo_index.get(&file_id)?.symbols;
            let qualified_name =
                qualified_name_for(&symbol, &all_symbols).or_else(|| Some(symbol.name.clone()));
            let snippet =
                read_symbol_snippet(&scope.repo_root, &file_id, &symbol).unwrap_or_default();
            Some(ToolMatch {
                path: file_id.0,
                snippet,
                score: 1.0,
                start_byte: symbol.start_byte,
                end_byte: symbol.end_byte,
                qualified_name,
                source: MatchSource::Symbol,
            })
        })
        .collect()
}

/// Best-effort slice of `[symbol.start_byte..symbol.end_byte)` off whatever
/// is on disk right now. Unlike `chunk_text::resolve_text`, there's no
/// per-symbol content hash to check staleness against (`Symbol` carries no
/// hash, only `IndexedFile.metadata.hash` does, at the whole-file level) —
/// this simply returns `None` (dropping the snippet, not the whole match)
/// if the byte range is no longer valid for the file's current content.
fn read_symbol_snippet(scope_root: &Path, file_id: &FileId, symbol: &Symbol) -> Option<String> {
    let path = scope_root.join(&file_id.0);
    let content = fs::read_to_string(&path).ok()?;
    let start = symbol.start_byte as usize;
    let end = symbol.end_byte as usize;
    if end > content.len()
        || start > end
        || !content.is_char_boundary(start)
        || !content.is_char_boundary(end)
    {
        return None;
    }
    Some(truncate_snippet(&content[start..end]))
}

fn truncate_snippet(text: &str) -> String {
    if text.len() <= SNIPPET_MAX_CHARS {
        return text.to_string();
    }
    let mut end = SNIPPET_MAX_CHARS;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Builds a `repo_root/docs/...` + `repo_root/src/...` fixture and
    /// returns `(repo_root, docs_root)`, both canonicalized. This file has
    /// far more parallel fixture-based tests than a nanosecond timestamp
    /// alone reliably disambiguates on a coarser system clock — the counter
    /// guarantees uniqueness within the process regardless of clock
    /// resolution.
    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_repo() -> (PathBuf, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let repo = std::env::temp_dir().join(format!("alfa-atlas-ai-tools-{nanos}-{n}"));
        let docs = repo.join("docs");
        let src = repo.join("src");
        fs::create_dir_all(&docs).unwrap();
        fs::create_dir_all(&src).unwrap();
        fs::write(docs.join("intro.adoc"), "= Intro\n").unwrap();
        fs::write(docs.join("script.py"), "print('unsupported ext')\n").unwrap();
        fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();

        let repo = repo.canonicalize().unwrap();
        let docs = docs.canonicalize().unwrap();
        (repo, docs)
    }

    /// Calls `execute_tool` for `ReadFile` and unwraps the expected
    /// `ToolResult::File` shape, so tests read like the plain `read_file`
    /// calls they replaced while still exercising the real public entry
    /// point (allowlist check included).
    fn read(scope: &ToolScope, path: &str) -> Result<String, ToolError> {
        match execute_tool(
            scope,
            ToolCall::ReadFile(ReadFileArgs {
                path: path.to_string(),
            }),
            &EmbeddingDeps::empty(),
        )? {
            ToolResult::File(content) => Ok(content),
            other => panic!("expected ToolResult::File, got {other:?}"),
        }
    }

    fn list(scope: &ToolScope, path: Option<&str>) -> Result<Vec<ToolFileEntry>, ToolError> {
        match execute_tool(
            scope,
            ToolCall::ListFiles(ListFilesArgs {
                path: path.map(str::to_string),
            }),
            &EmbeddingDeps::empty(),
        )? {
            ToolResult::FileList(entries) => Ok(entries),
            other => panic!("expected ToolResult::FileList, got {other:?}"),
        }
    }

    #[test]
    fn read_file_inside_docs_root_succeeds_in_docs_only() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let content = read(&scope, "intro.adoc").unwrap();
        assert_eq!(content, "= Intro\n");
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_on_a_directory_returns_not_a_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = read(&scope, ".").unwrap_err();
        assert!(matches!(err, ToolError::NotAFile(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_rejects_parent_escape_in_both_modes() {
        let (repo, docs) = fixture_repo();

        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = read(&docs_only, "../src/main.rs").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let err = read(&full_repo, "../outside.txt").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_same_relative_path_resolves_against_different_roots_by_mode() {
        let (repo, docs) = fixture_repo();

        // "src/main.rs" only exists under `repo`, not under `docs` — so the
        // same relative path is simply absent from the docs-only root
        // (there is no `docs/src/main.rs`), while it resolves fine once the
        // scope root widens to the whole repo. This is `ToolScope`'s mode
        // switch doing its job, not a `..`-escape (that's covered above).
        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        assert!(read(&docs_only, "src/main.rs").is_err());

        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let content = read(&full_repo, "src/main.rs").unwrap();
        assert_eq!(content, "fn main() {}\n");

        fs::remove_dir_all(&repo).ok();
    }

    /// `join_relative`'s `..`-rejection only catches lexical traversal; the
    /// real defense against a path that resolves outside the root by other
    /// means (e.g. a symlink) is `ensure_under`'s canonicalize+`starts_with`
    /// check. Exercise that directly so the containment guarantee isn't
    /// only proven for the lexical case.
    #[cfg(unix)]
    #[test]
    fn read_file_rejects_symlink_escaping_docs_root() {
        let (repo, docs) = fixture_repo();
        std::os::unix::fs::symlink(repo.join("src/main.rs"), docs.join("leak.adoc")).unwrap();

        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = read(&docs_only, "leak.adoc").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_docs_only_excludes_source_files() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let entries = list(&scope, None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"intro.adoc"));
        assert!(!paths.contains(&"script.py"));
        assert!(!paths.iter().any(|p| p.ends_with("main.rs")));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_full_repo_includes_source_files() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list(&scope, None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"docs/intro.adoc"));
        assert!(paths.contains(&"docs/script.py"));
        assert!(paths.contains(&"src/main.rs"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn execute_tool_denies_a_tool_missing_from_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let only_list: HashSet<ToolName> = [ToolName::ListFiles].into_iter().collect();
        let scope = ToolScope::new(&repo, &docs, AiAccessMode::DocsOnly, only_list);

        let err = read(&scope, "intro.adoc").unwrap_err();
        assert!(matches!(err, ToolError::NotAllowed(ToolName::ReadFile)));

        // The other tool in the same customized allowlist still works.
        assert!(list(&scope, None).is_ok());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn execute_tool_denies_semantic_search_missing_from_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let only_list: HashSet<ToolName> =
            [ToolName::ListFiles, ToolName::ReadFile].into_iter().collect();
        let scope = ToolScope::new(&repo, &docs, AiAccessMode::DocsOnly, only_list);

        let err = execute_tool(
            &scope,
            ToolCall::SemanticSearch(SemanticSearchArgs {
                query: "intro".to_string(),
                top_k: None,
            }),
            &EmbeddingDeps::empty(),
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::NotAllowed(ToolName::SemanticSearch)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn scope_for_config_defaults_to_both_tools_when_unset() {
        let (repo, docs) = fixture_repo();
        let config = ProjectConfig::new(".");

        let scope = scope_for_config(&repo, &docs, &config);
        assert!(read(&scope, "intro.adoc").is_ok());
        assert!(list(&scope, None).is_ok());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn scope_for_config_honors_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let mut config = ProjectConfig::new(".");
        config.ai_allowed_tools = Some(vec![ToolName::ListFiles]);

        let scope = scope_for_config(&repo, &docs, &config);
        assert!(matches!(
            read(&scope, "intro.adoc").unwrap_err(),
            ToolError::NotAllowed(ToolName::ReadFile)
        ));
        assert!(list(&scope, None).is_ok());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn tool_call_and_result_round_trip_through_json() {
        let call = ToolCall::ReadFile(ReadFileArgs {
            path: "intro.adoc".to_string(),
        });
        let json = serde_json::to_string(&call).unwrap();
        assert_eq!(json, r#"{"tool":"readFile","args":{"path":"intro.adoc"}}"#);
        let round_tripped: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, call);

        let result = ToolResult::File("= Intro\n".to_string());
        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(json, r#"{"tool":"file","result":"= Intro\n"}"#);
    }

    #[test]
    fn symbol_matches_finds_an_exact_case_insensitive_name() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/UserService.java"),
            "public class UserService {\n    public String getName() { return null; }\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(&repo_index, &scope, "userservice", 10);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].source, MatchSource::Symbol);
        assert!(matches[0].path.ends_with("UserService.java"));
        assert_eq!(matches[0].score, 1.0);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_is_empty_for_an_unknown_name() {
        let (repo, docs) = fixture_repo();
        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        assert!(symbol_matches(&repo_index, &scope, "NoSuchSymbol", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_excludes_non_doc_symbols_in_docs_only() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/UserService.java"),
            "public class UserService {\n    public String getName() { return null; }\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert!(symbol_matches(&repo_index, &scope, "userservice", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_finds_a_case_insensitive_substring() {
        use crate::domain::chunk_index::ChunkBuildOptions;
        use crate::services::chunk_builder::ChunkBuilder;

        let (repo, docs) = fixture_repo();
        fs::write(repo.join("docs/needle.adoc"), "= Guide\n\nfind the NEEDLE here\n").unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default()));
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = lexical_matches(&chunk_index, &scope, "needle", 10);
        assert!(!matches.is_empty());
        assert_eq!(matches[0].source, MatchSource::Lexical);
        assert!(matches[0].snippet.to_lowercase().contains("needle"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_is_empty_for_an_empty_query() {
        let (repo, docs) = fixture_repo();
        let chunk_index = ChunkIndex::new();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        assert!(lexical_matches(&chunk_index, &scope, "", 10).is_empty());
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_excludes_non_doc_chunks_in_docs_only() {
        use crate::domain::chunk_index::ChunkBuildOptions;
        use crate::services::chunk_builder::ChunkBuilder;

        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/Needle.java"),
            "public class Needle {\n    // find the NEEDLE here\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default()));
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert!(lexical_matches(&chunk_index, &scope, "needle", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }
}
