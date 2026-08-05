# AI Harness

Status of the AI-agent infrastructure in this app: what exists today, what it does, and what is deliberately not built yet.

## Status

**The tool-execution boundary is reachable from the frontend via one IPC command (`ai_execute_tool`); a standalone Repository Index + Chunk Index exist alongside it; and an Embedding Service (local BGE-M3 or a remote OpenAI-compatible API, backed by a `usearch` vector index) now turns chunks into searchable vectors and persists both the vectors and the chunk/file metadata needed to reload them — but no LLM is wired up and no chat UI exists yet.** This is groundwork for a future AI harness (an agent loop that calls an LLM with tool-calling) — specifically the trust boundary that decides *which files* and *which operations* that harness is allowed to touch, a per-file structural index, a semantic-chunk index, and now the embedding layer the `SemanticSearch` tool queries. The Rust side (`src-tauri/src`) is unit-tested throughout; the IPC command is additionally verified end-to-end (see "IPC surface" below) but has no caller anywhere in the app yet. Repository Index and Chunk Index still have no caller at all (not even IPC) beyond `embedding_sync` rebuilding them internally. The Embedding Service is the first layer in this file with real UI: a "Эмбеддинги" tab in Settings and a setup checklist in the RightDock "Ассистент" panel, both driving 11 `embedding_*` IPC commands and a live `embedding:sync-progress` event stream. `embedding_sync` is no longer purely manual: a dedicated file watcher (`services::index_watcher::IndexWatcher`, started as soon as a project opens) now reacts to on-disk changes to already-tracked files and keeps the index incrementally fresh between manual syncs, and a fresh project's very first sync prioritizes whichever files are open in the editor (plus their direct AsciiDoc includes/xrefs) over the rest of the repo, which is embedded afterward on a low-priority background task — see "Embeddings" below for both. A third tool, `SemanticSearch`, now sits in the harness alongside `ReadFile`/`ListFiles` and does consume the resulting vectors — with a three-tier degradation cascade (exact symbol-name match → semantic vector search when the index is ready → lexical grep fallback when it isn't) — but there is still no chat UI or LLM calling any of these tools.

**Memory/persistence architecture (added after the initial pass below was written):** `ChunkIndex` no longer holds chunk text resident in memory — only `ChunkMetadata` (byte offsets + hashes); text is read on demand from source files via `services::chunk_text::resolve_text`, and only for chunks a caller actually needs (a sync's pending/changed set, or — once it exists — a search's top-K results). `EmbeddingRecord` no longer duplicates the embedding vector (it only ever lived redundantly alongside `usearch`'s own copy). Both `ChunkIndex` and `EmbeddingIndex` mirror to a per-project SQLite store (`infra::index_store::IndexStore`, at `{project_root}/.atlas/index/{mode}/chunks.db` — **always** under the repo root, never under `docsRoot`, so `.atlas` stays the one place per-project state lives; `commands::embeddings::resolve_index_paths` is what computes this) plus a `vectors.usearch` file, so a restart reloads persisted state instead of re-walking/re-embedding from scratch, and `embedding_sync` re-chunks/re-embeds only files whose content hash actually changed since the last sync — see "Chunk Index" and "Embeddings" below for the details, and note that **the index is per-`AiAccessMode`**: `DocsOnly` and `FullRepo` walk different `index_root`s (`docsRoot` vs `repoRoot`) and are persisted under separate `{mode}` subfolders (`docs-only`/`full-repo`), so each has its own separate persisted store — switching the toggle does not share or invalidate the other mode's index, but a mode that has never been synced shows as empty until its own first `embedding_sync`.

The product vision for the user-facing feature this unblocks is documented separately in [`doc/business-requirements/02-functional-requirements.md`](doc/business-requirements/02-functional-requirements.md) (BR-4 "AI-ассистент документации", BR-5 "Подсказки AI").

## The two independent axes of restriction

A harness call is constrained along two orthogonal axes, both resolved into a single `ToolScope`:

1. **Root — `AiAccessMode`** ([`domain/ai_access.rs`](src-tauri/src/domain/ai_access.rs)): which part of the filesystem is visible at all.
   - `DocsOnly` (default) — only `docsRoot`, the documentation subtree.
   - `FullRepo` — the entire `repoRoot`, including source code.
2. **Tools — allowlist of `ToolName`** ([`domain/ai_access.rs`](src-tauri/src/domain/ai_access.rs)): which operations are callable at all, independent of the root. A project could be `FullRepo` but still have a given tool disabled.

Both are persisted per-project in `ProjectConfig` ([`domain/project_config.rs`](src-tauri/src/domain/project_config.rs), stored at `{repoRoot}/.atlas/project.json`):

```rust
pub struct ProjectConfig {
    pub docs_root: String,
    pub ai_access_mode: AiAccessMode,          // default: DocsOnly
    pub ai_allowed_tools: Option<Vec<ToolName>>, // None = use the mode's default allowlist
}
```

`ai_allowed_tools: None` means "not customized yet" — the effective set falls back to `default_allowed_tools(mode)`. Once a user sets an explicit list, it is authoritative: **adding a new `ToolName` variant later does not silently widen an already-customized allowlist** — a user has to opt a new tool in explicitly. This mirrors the existing safe-by-default choice of `AiAccessMode::DocsOnly`.

Legacy `project.json` files without these fields deserialize cleanly (`#[serde(default)]`) into the safe defaults — no migration needed.

## Module map

```
domain/ai_access.rs   — AiAccessMode, ToolName, default_allowed_tools(mode)   (policy/data)
domain/ai_tools.rs    — ToolCall, ToolResult, ToolScope, ToolError            (execution types)
domain/project_config.rs — ai_access_mode, ai_allowed_tools (persisted)
services/ai_tools.rs  — execute_tool(), scope_for_config(), current_scope()   (orchestration)
infra/workspace_scanner.rs — scan() / scan_all() (gitignore-aware file walk; scan_all skips the doc-format filter, used by FullRepo listing)
commands/ai_tools.rs  — ai_execute_tool (IPC command)
src/lib/aiTools.ts     — aiExecuteTool() (typed frontend wrapper, no callers yet)

domain/repo_index.rs        — Language, Symbol, FileMetadata, IndexedFile, LanguageIndexer trait, INDEX_VERSION
infra/language_indexers/*   — JavaIndexer, JsonIndexer, YamlIndexer, MarkdownIndexer, AsciiDocIndexer
services/repo_index.rs      — RepositoryIndex (build/get/file_ids/read/files_for_language/clear)

domain/chunk_index.rs       — ChunkSpan, ChunkStrategy trait, ChunkMetadata, Chunk (build-time only, carries text), CHUNK_VERSION, gap-ownership + splitting helpers
infra/chunk_strategies/*    — JavaChunkStrategy, MarkdownChunkStrategy, AsciiDocChunkStrategy, WholeFileChunkStrategy
services/chunk_builder.rs   — ChunkBuilder (build_file/build_all), ChunkIndex (metadata-only DashMap: insert_all/replace_for_file/chunks_for_file/get/get_with_text/file_hash_for/file_ids/chunk_ids/load_metadata/clear)
services/chunk_text.rs      — resolve_text(repo_root, &ChunkMetadata) — on-demand chunk text read off the source file, with Stale/OutOfBounds detection via file_hash

infra/index_store.rs           — IndexStore (SQLite: meta/files/chunks/embeddings tables — no text, no vectors)
services/index_store_ensure.rs — open_for(index_root) — opens/creates IndexStore, wipes+rebuilds meta on a CHUNK_VERSION/INDEX_VERSION/index_root mismatch

domain/embeddings.rs               — Embedding, EmbeddingRecord (chunk_hash only, no vector), EmbeddingProviderKind/Config, ModelStatus, SyncStats, EmbeddingIndexStatus, EmbeddingError, EmbeddingProvider trait
infra/embedding_providers/*        — LocalEmbeddingProvider (fastembed + BGE-M3 int8 ONNX), RemoteEmbeddingProvider (ureq, OpenAI-compatible /embeddings), provider_for()
infra/vector_store.rs              — VectorStore (usearch wrapper, new/load/save to disk), usearch_key(&ChunkId) -> u64
infra/embedding_credentials_store.rs — encrypted remote API key (AES-256-GCM, mirrors git_credentials_store.rs)
services/embedding_model.rs        — model_status(), download_model() (emits embedding:model-download-progress)
services/embedding_index.rs        — EmbeddingBuilder, EmbeddingIndex (new/load/sync w/ batched embed + progress callback + IndexStore write-through/get/search/clear)
services/embedding_config.rs       — load/save EmbeddingProviderConfig (AppSettings.embedding)
services/index_watcher.rs          — IndexWatcher (generic notify-based watcher: start/debounce/dispatch, decoupled from services::file_watcher::FileWatcher which only drives WorkspaceIndex)
commands/embeddings.rs             — 11 embedding_* IPC commands (incl. embedding_index_status, embedding_index_teardown, embedding_set_priority_files), EmbeddingIndexSlot, IndexStoreSlot, EmbeddingSyncGuard, IndexWatcherSlot, PriorityFilesSlot, emits embedding:sync-progress
src/lib/embeddings.ts              — typed wrappers + listenModelDownloadProgress()/listenSyncProgress()/setEmbeddingPriorityFiles()
src/hooks/useEmbeddingSetup.ts     — shared state (config/modelStatus/lastSync/indexStatus/syncProgress/backgroundSyncProgress) consumed by EmbeddingsTab and AssistantPanel
src/hooks/useEmbeddingIndexWarmup.ts — fire-and-forget embedding_index_status call on project open; also what starts/tears down the incremental IndexWatcher
src/hooks/useEmbeddingPriorityFiles.ts — reports open editor tabs to the backend (PriorityFilesSlot), so a fresh project's first sync prioritizes them
src/components/Settings/EmbeddingsTab.tsx   — provider choice, model download, API key, manual sync with live progress
src/components/RightDock/AssistantPanel.tsx — setup checklist (provider / model / sync); re-fetches indexStatus on AiAccessMode change since the index is per-mode
```

## The single entry point: `execute_tool`

Every tool call goes through one function in [`services/ai_tools.rs`](src-tauri/src/services/ai_tools.rs):

```rust
pub fn execute_tool(scope: &ToolScope, call: ToolCall) -> Result<ToolResult, ToolError>
```

`ToolCall`/`ToolResult` are enums, not separate per-tool functions — this is deliberate: one allowlist check, one place where a call/result gets serialized at the future LLM tool-calling boundary, and one place to eventually add logging (not implemented yet, but this is the seam for it). Adding a new tool means adding one match arm here, not a new public function with its own copy of the allowlist check.

```rust
pub enum ToolCall {
    ReadFile(ReadFileArgs),
    ListFiles(ListFilesArgs),
}

pub enum ToolResult {
    File(String),
    FileList(Vec<ToolFileEntry>),
}
```

Both derive `serde::Serialize`/`Deserialize` with an adjacently-tagged representation (`#[serde(tag = "tool", content = "args")]` / `content = "result"`), so the wire shape is fixed and tested:

```json
{"tool":"readFile","args":{"path":"intro.adoc"}}
```

`scope_for_config(repo_root, docs_root, config)` is the one place that turns a project's persisted config (or its absence) into a concrete `ToolScope`.

## Tools implemented today

All three are **read-only**; there is no write/edit tool yet — applying AI-suggested changes to a file is a future concern (see business-requirements BR-4.4/R-02/R-03 for the intended "preview + explicit confirm" UX).

- **`ReadFile { path }`** — reads one file's content, relative to the scope root.
- **`ListFiles { path? }`** — lists files under the scope root (or a subdirectory of it). In `DocsOnly` mode this reuses `services::docs_fs::list_docs_tree` (filtered to documentation formats, same as the sidebar tree); in `FullRepo` mode it uses `infra::workspace_scanner::scan_all` (gitignore-aware, no format filter, since source files are not documentation formats).
- **`SemanticSearch { query, topK? }`** (`services::ai_tools::semantic_search`) — a three-tier degradation cascade, each match tagged with which tier produced it (`source: "semantic" | "lexical" | "symbol"`, since scores aren't comparable across tiers) and carrying `path`/`snippet`/`score`/`startByte`/`endByte`/`qualifiedName`:
  1. **Symbol** (always tried first, cheapest — no I/O beyond a best-effort snippet read): `RepositoryIndex::find_symbol` exact case-insensitive name match, for "where is X defined" queries. Its hit count is subtracted from the `topK` budget passed to whichever tier runs next.
  2. **Semantic** (when the embedding index is ready): embeds the query via the configured `EmbeddingProvider`, calls `EmbeddingIndex::search` (cosine distance, converted to a `1 - distance` similarity score), resolves each hit's chunk text via `chunk_text::resolve_text`.
  3. **Lexical** (fallback, no embeddings at all): scans every `ChunkIndex` chunk's resolved text for a case-insensitive substring, ranked by occurrence count.
  Readiness (semantic vs. lexical) mirrors `embedding_index_status`'s own check (`resolve_index_paths` → `attach_index_store` → stale? → `attach_embedding_index(allow_repair: false)` → `embedded_count > 0`), plus a `try_lock` peek at `EmbeddingSyncGuard` for "a sync is actively running right now" — any failure anywhere in that sequence degrades to the lexical tier rather than erroring out, since this whole tool is a graceful cascade, not a pipeline that should hard-fail on a fast-path hiccup. `execute_tool`'s signature grew a third parameter, `deps: &EmbeddingDeps` (`services::ai_tools::EmbeddingDeps`, a bundle of `Arc`-cloned `RepositoryIndex`/`ChunkIndex`/`EmbeddingIndexSlot`/`IndexStoreSlot`/`EmbeddingProviderSlot`/`EmbeddingSyncGuard`), to reach this state — previously it (and `current_scope()`) were pure functions with zero Tauri-managed-state access.

## How access is actually enforced

Every path a tool touches is validated with `domain::paths::{join_relative, ensure_under}` — the same containment primitives `services::docs_fs` already uses for the editor's own file read/write commands. `join_relative` rejects `..` lexically; `ensure_under` canonicalizes and checks `starts_with(root)`, which also catches non-lexical escapes (e.g. a symlink inside the scope root pointing outside it — covered by a dedicated test). A caller can never widen access by passing an unusual path; the only way to see more is for the `ToolScope` itself to have been constructed with a wider root or a larger allowlist.

One known gap, intentionally not addressed by this tool layer: `commands/git.rs` (git diff/blob reads via `git2`) can already surface content from anywhere in the tracked repository, independent of `docsRoot` containment. A `DocsOnly`-mode harness must never be handed a git-diff-style tool — this is why the allowlist is an explicit opt-in table rather than "everything not yet disabled."

## IPC surface: `ai_execute_tool`

One Tauri command, in [`commands/ai_tools.rs`](src-tauri/src/commands/ai_tools.rs), registered in `generate_handler![]` in `lib.rs`:

```rust
#[tauri::command]
pub async fn ai_execute_tool(
    call: ToolCall,
    repo_index: State<'_, Arc<RepositoryIndex>>,
    chunk_index: State<'_, Arc<ChunkIndex>>,
    embedding_index: State<'_, Arc<EmbeddingIndexSlot>>,
    index_store: State<'_, Arc<IndexStoreSlot>>,
    embedding_provider: State<'_, Arc<EmbeddingProviderSlot>>,
    sync_guard: State<'_, Arc<EmbeddingSyncGuard>>,
) -> Result<ToolResult, String>
```
The six `State` params (added for `SemanticSearch`, all already `app.manage()`'d — no new registration needed) are cloned into an `EmbeddingDeps` before entering `spawn_blocking`, mirroring exactly how `commands::embeddings::embedding_sync` already receives its own `State<'_, Arc<T>>` params.

The frontend passes only `{ tool, args }` — it never passes `docsRoot`/`repoRoot`/an access mode, unlike every other document command in this codebase (`read_project_file(docsRoot, ...)` etc.). The command resolves which project is open itself, via a new `services::ai_tools::current_scope()`:

```rust
pub fn current_scope() -> Result<ToolScope, ProjectError>
```

`current_scope()` reuses `services::project_open::get_project()` (the same backend-authoritative "what's the current project" resolver `commands::project::get_project` uses at startup restore, reading `~/.atlas/settings.json`) to get `(repo_root, docs_root)`, then loads the full `ProjectConfig` via `infra::project_store::load` (for `ai_access_mode`/`ai_allowed_tools`, which `get_project()` alone discards), then calls `scope_for_config`. If no project is open, it returns an error whose message includes `"no project is open"`.

Runs via `spawn_blocking` (like `check_standards`) since `ListFiles` in `FullRepo` mode walks the whole repo.

Frontend wrapper, [`src/lib/aiTools.ts`](src/lib/aiTools.ts):
```ts
export function aiExecuteTool(call: ToolCall): Promise<ToolResult>
```
No `hooks/` layer and no UI wired to it yet — this is boundary-only, per AGENTS.md's `lib/` convention (one typed function per command, no logic).

Verified end-to-end against a running dev build (`bun run tauri dev`) via direct `window.__TAURI__.core.invoke` calls in the webview: `listFiles` returns the real docs tree, `readFile` returns real file content, and a `../`-traversal attempt is rejected with `"path escapes tool root: ..."` — confirming the containment boundary holds across the actual IPC channel, not just in Rust unit tests.

## Repository Index

A separate, standalone layer from the tool-execution boundary above: a structural per-file index — metadata, content hash, detected language, symbols. **No embeddings, no RAG, no chunking** — just an index a future tool/harness will query. Not wired into `ai_tools`/IPC/UI at all yet, and has no `#[tauri::command]` of its own; its only caller today is `commands::embeddings::embedding_sync`, which rebuilds it (a full walk) as the first step of every sync.

```rust
pub struct IndexedFile {
    pub metadata: FileMetadata,   // relative_path, size_bytes, modified_at: SystemTime, hash: blake3::Hash, language
    pub symbols: Vec<Symbol>,     // name, kind, start/end line, start/end byte
}
```

Deliberately **does not store file content** — a repo of thousands of files would otherwise duplicate the entire working tree in memory. Content is read separately when actually needed (the `ReadFile` tool, or a plain `fs::read_to_string`); the index describes the project, it doesn't duplicate the filesystem.

**Languages covered**: Java, JSON, YAML, Markdown, AsciiDoc (`domain::repo_index::Language`) — matches the Java/Kotlin-backend-with-JSON/YAML-schemas repos this app documents, minus Kotlin (see below). `detect_language` is extension-based; anything else (including this app's own `.rs`/`.ts`) is skipped entirely.

**Per-language indexing** (`infra/language_indexers/`), registered once and explicitly in `default_indexers() -> HashMap<Language, Arc<dyn LanguageIndexer>>` — no `supports()` self-reporting, so the language↔indexer mapping exists in exactly one place:
- **Java** — real parsing via `tree-sitter`/`tree-sitter-java`: class/interface/enum/method/constructor/field declarations, each with full line+byte ranges straight off the tree-sitter node. This is a documented swap point — a future `JdtLsJavaIndexer: LanguageIndexer` replaces it by changing one line in `default_indexers()`.
- **AsciiDoc** — real parsing via `tree-sitter`/`tree-sitter-asciidoc`: section titles (`document_title`, `title1`..`title5` nodes). Chosen over a hand-written line scan specifically because a line scan can't distinguish a real section title from an `=` that merely starts a line inside a listing block or table — a grammar-aware parser doesn't have that failure mode (see `infra/language_indexers/asciidoc.rs`'s doc comment for the specific test that proves it, `ignores_equals_signs_inside_listing_blocks`).
- **Markdown** — `pulldown-cmark` event walk; every heading becomes a symbol (unlike `infra/parsers/markdown.rs`'s anchor extraction, which only records a heading with an explicit `{ #id }`).
- **JSON / YAML** — deliberately extract **zero symbols**. A regex key-scan produces false positives on quoted values that merely contain a colon (`{"query": "field:value"}`), and `serde_json`/`serde_yaml` carry no position information to do this properly. The file is still indexed (metadata + hash + language); only `.symbols` is empty, on principle, rather than guessing.

**Kotlin was dropped entirely.** `tree-sitter-kotlin` (the only Kotlin grammar crate on crates.io) is capped at `tree-sitter <0.23`. Once AsciiDoc was prioritized and `tree-sitter-asciidoc`'s compiled grammar turned out to need language ABI 15 (unsupported by tree-sitter 0.22's runtime — Cargo's `links = "tree-sitter"` only allows one `tree-sitter` version project-wide), `tree-sitter` was bumped to `0.26.11` and Kotlin support was removed rather than kept on an incompatible pin. Re-add `Language::Kotlin` if a `tree-sitter-kotlin` release ever raises its own upper bound.

**Resilience policy** (`services::repo_index::RepositoryIndex::build`): an unreadable file (I/O error, non-UTF-8) is skipped entirely with a warning. A file that reads fine but is malformed for its language (e.g. broken Java) still gets a full record — `LanguageIndexer::index` is infallible by signature, so a broken file's `symbols` may come back short or empty, but the file never disappears from the index.

`INDEX_VERSION` (currently `1`) — bumped whenever indexer behavior changes in a way that would change output for the same input — is exactly the staleness signal `services::index_store_ensure::open_for` now checks against a persisted store's `index_version` meta row before trusting it (see "Chunk Index" below); `RepositoryIndex` itself still doesn't persist (it's rebuilt by a full walk on every `embedding_sync`), only `ChunkIndex`/`EmbeddingIndex` are mirrored to disk.

## Chunk Index

Built on top of Repository Index: `RepositoryIndex → ChunkBuilder → ChunkIndex`. **Still no embeddings, no BGE-M3, no vector DB** — this layer only splits an already-indexed file into meaningful, addressable text fragments (a method, a doc section, a whole small file). That split is what a future embeddings stage would actually run over.

```rust
pub struct Chunk {
    pub metadata: ChunkMetadata,  // id, file_id, language, kind, byte range, file_hash, hash, qualified_name, ordinal
    pub text: String,             // unlike IndexedFile, a Chunk DOES carry its own text — it's the unit of work
}
```

**Gap ownership** decides which bytes belong to which chunk without a useless `ChunkKind::Other`/`Gap`: for Java, `Method`/`Field` symbols already capture their own full body, so each chunk absorbs the *gap before it* (annotations, Javadoc, blank lines, and for the first chunk — package/imports/class declaration too); the last chunk also absorbs the file's trailing suffix. For Markdown/AsciiDoc, heading symbols only mark their own title line, so each chunk absorbs the content *forward* to the next heading (or EOF). JSON/YAML (no symbols) — and any file whose language has symbols but yields none, e.g. an empty class — become one `ChunkKind::File` chunk. `ChunkKind` is exactly `Method | Field | Section | File`.

**Oversized chunks** (`> DEFAULT_MAX_CHUNK_BYTES`, 16KB) are split as a separate, uniform pass after semantic splitting — never per-language — preferring a blank line, `;`, or `}` boundary within a lookback window over an arbitrary cut, always on a valid UTF-8 char boundary.

**Identity vs. change detection are deliberately separate fields**: `ChunkId` is the human-readable `"{file_id}#{start_byte}-{end_byte}"`; `hash` is `BLAKE3(file_hash || start_byte || end_byte || CHUNK_VERSION)` — derived from position, not from re-hashing `text`, so a later embeddings stage can ask "did this exact chunk change?" cheaply. `file_hash` is copied onto every chunk (not just reachable via `RepositoryIndex`) since a `ChunkIndex` will often outlive or travel separately from the index that built it — e.g. once chunks are written to a Vector DB.

**`ChunkBuilder` and `ChunkIndex` are separate types** — the builder holds only the per-language `ChunkStrategy` registry and produces `Vec<Chunk>` (with text); the index stores only `DashMap<ChunkId, ChunkMetadata>` — `insert_all`/`replace_for_file` take builder output but drop `text` immediately after extracting `metadata`. `build_file`/`build_all` already exist as two entry points (`build_all` just loops `build_file` today) specifically so a future file watcher can call `build_file` + `chunk_index.replace_for_file` for exactly the one changed file without either type needing to change; `commands::embeddings::embedding_sync` already does this today for its own incremental diff (see "Embeddings" below), just not yet driven by a watcher.

A `ChunkStrategy` only returns ranges (`ChunkSpan`) — hashing, `qualified_name` lookup (smallest enclosing `Class`/`Interface`/`Enum` symbol, e.g. `"UserService.save"`), and `ordinal` assignment all happen once in `ChunkBuilder`, not duplicated per language.

**Text is not resident in `ChunkIndex`.** Storing every chunk's text (up to `DEFAULT_MAX_CHUNK_BYTES`, 16KB, each) for a whole indexed tree would make resident memory scale with the total size of the indexed text, not with chunk count — the same duplication `IndexedFile` already avoids one layer down. `services::chunk_text::resolve_text(repo_root, &ChunkMetadata)` reads `[start_byte..end_byte)` straight off the source file instead, first checking that the file's current content still hashes to `metadata.file_hash` (`ChunkTextError::Stale` if not — the file changed or was deleted since indexing) before trusting the byte range. `ChunkIndex::get_with_text` wraps this for callers that need both metadata and text in one call. Transparently cached: a process-wide `moka::sync::Cache<blake3::Hash, Arc<str>>` (a `static OnceLock`, module-private to `chunk_text.rs` — `resolve_text`'s signature and every one of its callers are unaware caching happens) keyed on `ChunkMetadata.hash` — not `ChunkId` — since `hash` already encodes `file_hash`/`start_byte`/`end_byte`/`CHUNK_VERSION`, so a content change, a position shift, or a chunking-version bump all naturally produce a different key; a stale entry under an old `hash` is simply never looked up again, no explicit invalidation needed. Bounded by memory, not entry count (chunks range up to 16KB each): a `weigher` closure reports each entry's byte length, and `max_capacity` (64MiB) is a weighted-size budget, not a count. Only successful resolutions are cached — a `ChunkTextError` is always retried fresh next call. The practical effect: `EmbeddingIndex::sync` only ever reads text for chunks that are new or changed (see "Embeddings"), and the `SemanticSearch` tool's semantic/lexical tiers only read it for a query's top-K results (or every chunk, for the lexical fallback's scan) — never resident for the whole corpus at once.

**Persistence**: `ChunkIndex`'s metadata is mirrored to the `chunks`/`files` tables of a per-project `infra::index_store::IndexStore` (SQLite, at `{project_root}/.atlas/index/{mode}/chunks.db` — always anchored at the repo root, in a `docs-only`/`full-repo` subfolder per `AiAccessMode`, never under `docsRoot`) — `ChunkMetadata` in, `ChunkMetadata` out (`IndexStore::load_all_chunks`), no text. `ChunkIndex::load_metadata` bulk-repopulates the resident `DashMap` from that on a cold start (a new `index_root` this process hasn't seen yet — first sync since app launch, or a project/access-mode switch), so the metadata a diff needs (`file_hash_for`, `file_ids`) survives a restart without a repo rescan. `services::index_store_ensure::open_for(storage_dir, index_root)` is the read-only guard in front of this: it compares the store's persisted `chunk_version`/`index_version`/`index_root` meta rows against the running binary's `CHUNK_VERSION`/`repo_index::INDEX_VERSION` and the freshly-resolved `index_root`, reporting a mismatch as `stale` rather than repairing it inline — repairing (wiping the store and the sibling `vectors.usearch`) only happens via `index_store_ensure::repair_stale`, called from within a real `embedding_sync`, never from a read-only attach/status path (see "Embeddings" below for why: an eager status check must never block on a synchronous wipe).

## Embeddings

Built on top of Chunk Index: `ChunkIndex → EmbeddingBuilder → EmbeddingIndex (usearch)`. This is the layer the `SemanticSearch` tool's semantic tier actually queries (see "Tools implemented today") — everything before it only produces addressable text, nothing before it produces a vector.

```rust
pub trait EmbeddingProvider: Send + Sync {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError>;  // batched
    fn dimensions(&self) -> usize;
}
```

**Two providers, one persisted choice** (`AppSettings.embedding: EmbeddingProviderConfig`, global — not per-project):
- **Local** (default) — `infra::embedding_providers::local::LocalEmbeddingProvider`, wrapping `fastembed`'s BGE-M3 (int8 ONNX, 1024-dim, HF repo `gpahal/bge-m3-onnx-int8`). Chosen for multilingual strength (this app's primary content language is Russian). Not bundled — `~570MB`, downloaded on demand and cached at `~/.atlas/models` on first use, same "fetch once, cache locally" shape as any `hf-hub`-backed tool.
- **Remote** — `infra::embedding_providers::remote::RemoteEmbeddingProvider`, a full (not stub) OpenAI-compatible client: `POST {base_url}/embeddings` with `{"input":[...],"model":...}`, `Authorization: Bearer {key}`, parses `{"data":[{"embedding":[...]}]}` — works against OpenAI, Together, Mistral, or a local Ollama/LM Studio server. The API key is never stored in `settings.json`; it goes through `infra::embedding_credentials_store` (AES-256-GCM, the same encryption key `key_management` already uses for the Git SSH private key).

Both are synchronous (`fastembed` has no Tokio dependency; the remote client uses blocking `ureq` rather than expanding this project's minimal `tokio` feature set) — callers run `embed()` inside `spawn_blocking`, same as `check_standards`/`ai_execute_tool`.

**Model download** (`services::embedding_model`): `model_status()` reports `NotDownloaded | Downloading{progress} | Ready | Error` by checking `hf-hub`'s cache directory without triggering a fetch; `download_model(app_handle)` triggers it and emits `embedding:model-download-progress` events. **Known limitation**: `fastembed` does not expose a byte-level download callback through its own `InitOptions` API (confirmed by reading `hf-hub`'s source — the granular hook exists there but isn't threaded through), so progress is coarse: one `0.0` event at start, one `1.0` event at completion, not a smooth bar.

**Vector storage**: `infra::vector_store::VectorStore` wraps `usearch` (embedded HNSW/ANN, no server process, `MetricKind::Cos`). `usearch` keys entries by `u64`, not `ChunkId`, so `usearch_key(chunk_id)` derives one deterministically (`u64::from_le_bytes(blake3(chunk_id)[..8])`); `EmbeddingIndex` keeps a `key_to_chunk: DashMap<u64, ChunkId>` reverse map so `search()` can translate results back. `RepositoryIndex`/`ChunkIndex` hold every piece of chunk/file metadata a search result needs to be resolved back to a file and byte range.

`VectorStore::save(path)`/`VectorStore::load(dimensions, path)` persist the index to/from a `vectors.usearch` file — `load()`, deliberately not `usearch`'s mmap-backed `view()`, because the index must stay mutable for `upsert`/`remove` across the app's lifetime (a `view()`ed index is read-only by design). A `VectorStore` built via `load()` remembers its path so a later `clear()` also deletes the on-disk file (avoids a stale, wrong-dimension file surviving a rebuild) and so `EmbeddingIndex::sync` knows to `save()` again after mutating.

**Incremental sync** (`EmbeddingIndex::sync`, `services/embedding_index.rs`) does all three rules from the original request in one reconciliation pass over `chunk_index.chunk_ids()` vs. its own `records: DashMap<ChunkId, EmbeddingRecord>`:
- No record for a `ChunkId` → **new chunk**, its text resolved via `chunk_text::resolve_text` and queued.
- Record exists but `record.chunk_hash != chunk.metadata.hash` → **changed**, same treatment (chunk `hash` already encodes "did this span's content or position change" from the Chunk Index stage — `sync` doesn't need its own staleness logic). Unchanged chunks never have their text read at all.
- A `ChunkId` in `records` no longer present in `chunk_index.chunk_ids()` → **deleted**, removed from both `records` and the vector store.

Queued (new/changed) chunks are embedded in batches of `EMBED_PROGRESS_BATCH` (32) — one `provider.embed(...)` call per batch, not one call for the whole pending set — so an optional `on_progress: Option<&dyn Fn(usize, usize)>` callback can report `(embedded_so_far, total_pending)` between batches; `commands::embeddings::embedding_sync` turns that into `embedding:sync-progress` events (see below). An optional `store: Option<&IndexStore>` argument is written through at the same points `records` mutates (`upsert_embedding`/`delete_embedding`), and the vector index is `save()`d back to its `load`ed path once at the end if anything changed — a full-file `usearch` write per `sync()` call, not per chunk.

`EmbeddingIndexSlot` (`Mutex<Option<(PathBuf, usize, EmbeddingIndex)>>`, `commands/embeddings.rs`) is keyed by `(index_root, dimensions)`, not dimensions alone — either a different project/access-mode (`index_root` changes) or a different provider/model (`dimensions` changes) invalidates the resident index. On a mismatch, `attach_embedding_index` doesn't always rebuild blank: it first checks the store's persisted `embedding_dimensions` meta value — if it matches the now-current `dimensions`, `EmbeddingIndex::load` reloads `vectors.usearch` + the SQLite `chunk_hash` mirror (`IndexStore::load_all_embedding_hashes`) instead of starting empty; if it doesn't (first sync ever, or the provider's dimension changed since last time), whatever's on disk for that mismatched dimension is dropped (`clear_embeddings` + delete `vectors.usearch`) rather than risked.

`IndexStoreSlot` (`Mutex<Option<(PathBuf, Arc<IndexStore>, bool)>>`) is the third such lazily-attached slot — one SQLite connection per `index_root`, shared by `ChunkIndex`'s cold-start reload and `EmbeddingIndex`'s persistence; the `bool` caches whether that attach was `stale` (see below) so later calls in the same session don't need to re-derive it, and `embedding_sync` flips it to `false` in place once it actually repairs a stale store. `commands::embeddings::attach_index_store`/`attach_embedding_index` are the two helpers both `embedding_sync` and `embedding_index_status` build on to reach this state without duplicating the attach-or-reload logic; `attach_embedding_index` takes an `allow_repair` flag so only `embedding_sync` (already mutating) can destructively drop mismatched-dimension `vectors.usearch`/`embeddings` rows — `embedding_index_status`'s read-only call just reports an empty index for that dimension without touching disk.

**Incremental file-watcher sync**: `services::index_watcher::IndexWatcher` (a generic `notify`-based watcher — root, debounce duration, and an injected `is_relevant`/`on_change` closure pair, decoupled from `services::file_watcher::FileWatcher`, which is a separate, older watcher hardcoded to `WorkspaceIndex`) is started for `index_root` by `ensure_incremental_watcher`, called from both `embedding_sync` and the read-only `embedding_index_status` — the latter is what makes watching begin the moment a project opens (`useEmbeddingIndexWarmup`), not only after the user's first manual sync. Its `on_change` reaction, `run_incremental_sync`, re-chunks and re-embeds exactly the one changed file (via `RepositoryIndex::update_file`/`remove_file`, new single-file counterparts to `build()`) — but only for a file `RepositoryIndex` already tracks; a genuinely new file still waits for the next full/manual `embedding_sync`, which does the real gitignore-aware walk. `EmbeddingSyncGuard` (`Mutex<()>`, `commands/embeddings.rs`) serializes every full sync against every incremental tick (and, see below, the background backlog) so their multi-step read-then-write sequences over `RepositoryIndex`/`ChunkIndex`/`IndexStore`/`EmbeddingIndex` never interleave. `embedding:sync-progress` payloads carry a `trigger: "full" | "incremental" | "background"` field precisely so the UI (`useEmbeddingSetup`) can tell a real user-triggered sync apart from a quiet background tick and never show the wrong current/total numbers. `embedding_index_teardown` stops the watcher when a project closes without a new one opening in the same session (`ensure_incremental_watcher` itself only swaps it when a *different* project's `index_root` shows up).

**Open-files-first prioritization on a fresh project's first sync**: `embedding_sync` detects "first sync" as `chunk_index.chunk_ids().is_empty()` — every other call (routine re-sync, incremental tick) is untiered, exactly as before. On a first sync, `PriorityFilesSlot` (`Mutex<HashSet<FileId>>`, populated by the frontend calling `embedding_set_priority_files` with the currently open editor tabs' paths — `src/hooks/useEmbeddingPriorityFiles.ts`, fired on tab open/close) is expanded one hop via `WorkspaceIndex::find_includes`/`find_references` (the existing AsciiDoc include/xref graph, bridged from its repo-root-relative `DocumentId` space into `FileId`'s `index_root`-relative space by a plain string-prefix helper — no filesystem calls) to get the priority set: open files plus their direct includes/xrefs. That set is chunked+embedded inline so the command returns quickly; everything else is handed to `run_background_backlog_sync`, a detached task (`tauri::async_runtime::spawn_blocking`, not awaited by the command) that works through the rest in small batches, acquiring `EmbeddingSyncGuard` per batch (not once for the whole backlog) so a later manual sync or incremental tick can still interleave rather than wait out the entire first-sync backlog. `EmbeddingIndexStatus` gained a `background_pending: usize` field for this — `repo_index.file_ids().len() - chunk_index.file_ids().len()`, i.e. files the repo walk found but hasn't chunked yet — derived from live state rather than a hand-kept counter, so it survives a restart or a panicked background task without drifting; the UI (`AssistantPanel`/`EmbeddingsTab`) shows it as a small non-blocking note, never gating the sync action or the "completed" checklist state on it reaching zero.

**IPC surface** (`commands/embeddings.rs`, 11 commands): `embedding_get_config`/`embedding_set_config`, `embedding_set_remote_api_key`/`embedding_has_remote_api_key` (write-only key, mirrors `git_save_credentials`), `embedding_model_status`, `embedding_download_model` (async, streams `embedding:model-download-progress`), `embedding_cancel_model_download`, `embedding_sync` (async — full `RepositoryIndex.build()` rescan every call, but re-chunks only files whose hash changed since `ChunkIndex` last saw them via `ChunkIndex::file_hash_for`, then `EmbeddingIndex::sync`, tiered into priority/background on a fresh project's first sync (see above); emits `embedding:sync-progress` events — `phase: "chunking"|"embedding"`, `trigger: "full"|"incremental"|"background"` — while running; returns `SyncStats{embedded, skipped_unchanged, removed}` for whichever tier ran inline), **`embedding_index_status`** (async, read-only — attaches/cold-start-reloads the same state `embedding_sync` would use, but never walks the repo, never repairs a stale store, and never constructs a real `EmbeddingProvider`: dimensions come from `embedding_providers::expected_dimensions(&config)`, a plain config read, specifically so this stays cheap for the Local provider too — the naive `provider_for(...).dimensions()` would otherwise load the whole ~570MB ONNX model just to read a constant; also starts the incremental watcher, see above; returns `EmbeddingIndexStatus{synced, embeddedCount, stale, backgroundPending}` derived from the resident/persisted index itself, not from whether a sync happened to run earlier in this process — `stale: true` means a persisted index exists but predates a version bump and needs a real `embedding_sync` to repair, distinct from "never synced"), `embedding_index_teardown`, and `embedding_set_priority_files` (see above). `App.tsx` calls `embedding_index_status` eagerly via `useEmbeddingIndexWarmup` (`src/hooks/useEmbeddingIndexWarmup.ts`) as soon as a project opens — a fire-and-forget call purely so the backend attach happens immediately rather than waiting for the user to open a specific panel; it stores no state of its own. Frontend wrappers in `src/lib/embeddings.ts` (incl. `listenSyncProgress`, `setEmbeddingPriorityFiles`), shared state in `src/hooks/useEmbeddingSetup.ts` (`indexStatus`, refreshed on mount and after every `sync()`, is what lets the UI show real "already built"/"stale"/"background catch-up in progress" state across a component remount instead of resetting to "not yet synced").

**This is the first layer in this file with real UI**: a "Эмбеддинги" tab in Settings (provider choice, model download with a live progress bar, remote base URL/model/API key, manual sync trigger with live chunking/embedding progress) and a setup checklist in the RightDock "Ассистент" panel (`AssistantPanel.tsx`, replacing what had been an empty stub since the very first AI-harness stage) — "Настроить провайдера" / "Загрузить модель" (Local only) / "Синхронизировать индекс", each item clickable, both consuming the same `useEmbeddingSetup` hook so they never disagree about current state. Unlike the other two (genuinely one-time) checklist items, "Синхронизировать индекс" keeps its action button visible even once `completed` (`alwaysShowAction`) — docs keep changing, so re-sync must stay reachable, not permanently hidden behind a "Готово" badge. `AssistantPanel` also re-`refresh()`s `useEmbeddingSetup`'s state (including `indexStatus`) whenever the `AiAccessMode` toggle changes, since the index is per-mode and would otherwise keep showing whichever mode's count was last fetched.

## What is not built yet

- No UI to view or change `ai_access_mode` / `ai_allowed_tools` — currently editable only by hand-editing `project.json` or in Rust code/tests.
- No LLM client, no chat UI, no streaming — this file only governs *what a future harness may read*, not how it talks to a model.
- No write/edit tool.
- No logging at the `execute_tool` call site (the single entry point exists specifically so this is a one-line addition later).
- No frontend caller of `aiExecuteTool` yet — the IPC boundary is wired and verified, but nothing in the UI triggers it.
- Neither Repository Index nor Chunk Index has a `#[tauri::command]` of its own or any wiring into `ai_tools`'s `ToolCall`/`ToolName` — both are only reachable indirectly, via `embedding_sync` rebuilding/diffing them internally.
- `RepositoryIndex::build()` (the *full*, `embedding_sync`-driven rescan) still does a full walk + re-hash + re-parse of every file every time it runs — cheap relative to embedding inference (no network/ONNX), so not yet worth diffing further, but there's no `mtime`/size pre-filter to skip re-hashing an untouched file either. `RepositoryIndex` does now also have single-file `update_file`/`remove_file` methods (used by the incremental watcher below, one file per call, no full walk) — but a full sync's own diff loop still re-derives its changed-file set from a fresh `build()`, not from watching.
- No `Symbol` persistence — only chunk/file metadata (offsets + hashes) is mirrored to SQLite, so a restart still re-parses (tree-sitter/pulldown-cmark) every file's symbols even though it skips re-embedding unchanged chunks; deliberately deferred (parsing is fast relative to inference, and persisting symbols would add its own versioning surface for tree-sitter grammar upgrades).
- File-watcher-driven incremental sync exists now (`services::index_watcher::IndexWatcher`, see "Embeddings" above) but only for files `RepositoryIndex` already tracks — a brand-new file created on disk is invisible to it until the next full/manual `embedding_sync` (the incremental path deliberately never does the gitignore-aware walk a new-file discovery would need). A fresh project's first sync also only prioritizes files currently open in the editor (plus their direct AsciiDoc includes/xrefs) — there is no dependency-graph equivalent for Java/JSON/YAML files (no import/reference graph exists for those languages yet), so an open non-AsciiDoc file gets no expansion, just itself.
- `SemanticSearch` (see "Tools implemented today") has no UI or LLM caller yet — like `ReadFile`/`ListFiles`, it's reachable via `ai_execute_tool` and unit-tested, but nothing in the app invokes it outside tests.
- Model download progress is still coarse (0%/100% only) — see "Embeddings" above for why; this is unrelated to and unaffected by `embedding_sync`'s own (now granular, batch-level) `embedding:sync-progress` events.

## Extending this

Adding a new tool touches exactly these spots:
1. `domain/ai_access.rs`: add the `ToolName` variant, decide whether it belongs in `default_allowed_tools`.
2. `domain/ai_tools.rs`: add args/result types if needed, a `ToolCall`/`ToolResult` variant, update `ToolCall::name()`.
3. `services/ai_tools.rs`: implement the tool as a private function, add one match arm in `execute_tool`.
4. If the tool can read content outside `ensure_under`'s reach (like the git-diff case above), do **not** add it to `default_allowed_tools` — require explicit per-project opt-in.
