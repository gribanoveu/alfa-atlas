# AI Harness

Status of the AI-agent infrastructure in this app: what exists today, what it does, and what is deliberately not built yet.

## Status

**The tool-execution boundary is reachable from the frontend via one IPC command (`ai_execute_tool`); a standalone Repository Index + Chunk Index exist alongside it; and an Embedding Service (local BGE-M3 or a remote OpenAI-compatible API, backed by a `usearch` vector index) now turns chunks into searchable vectors — but no LLM is wired up and no chat UI exists yet.** This is groundwork for a future AI harness (an agent loop that calls an LLM with tool-calling) — specifically the trust boundary that decides *which files* and *which operations* that harness is allowed to touch, a per-file structural index, a semantic-chunk index, and now the embedding layer a future semantic-search/RAG tool will query. The Rust side (`src-tauri/src`) is unit-tested throughout; the IPC command is additionally verified end-to-end (see "IPC surface" below) but has no caller anywhere in the app yet. Repository Index and Chunk Index still have no caller at all (not even IPC). The Embedding Service is the first layer in this file with real UI: a "Эмбеддинги" tab in Settings and a setup checklist in the RightDock "Ассистент" panel, both driving the same 7 `embedding_*` IPC commands — but nothing calls `embedding_sync` automatically yet (no file-watcher), and there is still no semantic-search command/UI consuming the resulting vectors.

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

domain/chunk_index.rs       — ChunkSpan, ChunkStrategy trait, ChunkMetadata, Chunk, CHUNK_VERSION, gap-ownership + splitting helpers
infra/chunk_strategies/*    — JavaChunkStrategy, MarkdownChunkStrategy, AsciiDocChunkStrategy, WholeFileChunkStrategy
services/chunk_builder.rs   — ChunkBuilder (build_file/build_all), ChunkIndex (insert_all/replace_for_file/chunks_for_file/get/clear/chunk_ids)

domain/embeddings.rs               — Embedding, EmbeddingRecord, EmbeddingProviderKind/Config, ModelStatus, SyncStats, EmbeddingError, EmbeddingProvider trait
infra/embedding_providers/*        — LocalEmbeddingProvider (fastembed + BGE-M3 int8 ONNX), RemoteEmbeddingProvider (ureq, OpenAI-compatible /embeddings), provider_for()
infra/vector_store.rs              — VectorStore (usearch wrapper), usearch_key(&ChunkId) -> u64
infra/embedding_credentials_store.rs — encrypted remote API key (AES-256-GCM, mirrors git_credentials_store.rs)
services/embedding_model.rs        — model_status(), download_model() (emits embedding:model-download-progress)
services/embedding_index.rs        — EmbeddingBuilder, EmbeddingIndex (sync/get/search/clear)
services/embedding_config.rs       — load/save EmbeddingProviderConfig (AppSettings.embedding)
commands/embeddings.rs             — 7 embedding_* IPC commands, EmbeddingIndexSlot
src/lib/embeddings.ts              — typed wrappers + listenModelDownloadProgress()
src/hooks/useEmbeddingSetup.ts     — shared state consumed by EmbeddingsTab and AssistantPanel
src/components/Settings/EmbeddingsTab.tsx   — provider choice, model download, API key, manual sync
src/components/RightDock/AssistantPanel.tsx — setup checklist (provider / model / sync)
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

Both are **read-only**; there is no write/edit tool yet — applying AI-suggested changes to a file is a future concern (see business-requirements BR-4.4/R-02/R-03 for the intended "preview + explicit confirm" UX).

- **`ReadFile { path }`** — reads one file's content, relative to the scope root.
- **`ListFiles { path? }`** — lists files under the scope root (or a subdirectory of it). In `DocsOnly` mode this reuses `services::docs_fs::list_docs_tree` (filtered to documentation formats, same as the sidebar tree); in `FullRepo` mode it uses `infra::workspace_scanner::scan_all` (gitignore-aware, no format filter, since source files are not documentation formats).

## How access is actually enforced

Every path a tool touches is validated with `domain::paths::{join_relative, ensure_under}` — the same containment primitives `services::docs_fs` already uses for the editor's own file read/write commands. `join_relative` rejects `..` lexically; `ensure_under` canonicalizes and checks `starts_with(root)`, which also catches non-lexical escapes (e.g. a symlink inside the scope root pointing outside it — covered by a dedicated test). A caller can never widen access by passing an unusual path; the only way to see more is for the `ToolScope` itself to have been constructed with a wider root or a larger allowlist.

One known gap, intentionally not addressed by this tool layer: `commands/git.rs` (git diff/blob reads via `git2`) can already surface content from anywhere in the tracked repository, independent of `docsRoot` containment. A `DocsOnly`-mode harness must never be handed a git-diff-style tool — this is why the allowlist is an explicit opt-in table rather than "everything not yet disabled."

## IPC surface: `ai_execute_tool`

One Tauri command, in [`commands/ai_tools.rs`](src-tauri/src/commands/ai_tools.rs), registered in `generate_handler![]` in `lib.rs`:

```rust
#[tauri::command]
pub async fn ai_execute_tool(call: ToolCall) -> Result<ToolResult, String>
```

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

A separate, standalone layer from the tool-execution boundary above: a structural per-file index — metadata, content hash, detected language, symbols. **No embeddings, no RAG, no chunking** — just an index a future tool/harness will query. Not wired into `ai_tools`/IPC/UI at all yet; it's pure Rust (`domain`/`infra`/`services`), unit-tested, with no caller anywhere in the app.

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

`INDEX_VERSION` (currently `1`) exists from day one even though nothing persists the index yet — bumped whenever indexer behavior changes in a way that would change output for the same input, so a future on-disk cache has a cheap staleness signal instead of a retrofit.

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

**`ChunkBuilder` and `ChunkIndex` are separate types** — the builder holds only the per-language `ChunkStrategy` registry and produces `Vec<Chunk>`; the index just stores (`DashMap<ChunkId, Chunk>`). `build_file`/`build_all` already exist as two entry points (`build_all` just loops `build_file` today) specifically so a future file watcher can call `build_file` + `chunk_index.replace_for_file` for exactly the one changed file without either type needing to change.

A `ChunkStrategy` only returns ranges (`ChunkSpan`) — hashing, `qualified_name` lookup (smallest enclosing `Class`/`Interface`/`Enum` symbol, e.g. `"UserService.save"`), and `ordinal` assignment all happen once in `ChunkBuilder`, not duplicated per language.

## Embeddings

Built on top of Chunk Index: `ChunkIndex → EmbeddingBuilder → EmbeddingIndex (usearch)`. This is the layer a future semantic-search/RAG tool will actually query — everything before it only produces addressable text, nothing before it produces a vector.

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

**Vector storage**: `infra::vector_store::VectorStore` wraps `usearch` (embedded HNSW/ANN, no server process, `MetricKind::Cos`). `usearch` keys entries by `u64`, not `ChunkId`, so `usearch_key(chunk_id)` derives one deterministically (`u64::from_le_bytes(blake3(chunk_id)[..8])`); `EmbeddingIndex` keeps a `key_to_chunk: DashMap<u64, ChunkId>` reverse map so `search()` can translate results back. No SQLite this pass — `RepositoryIndex`/`ChunkIndex` already hold every piece of chunk/file metadata a search result needs to be resolved back to a file and byte range.

**Incremental sync** (`EmbeddingIndex::sync`, `services/embedding_index.rs`) does all three rules from the original request in one reconciliation pass over `chunk_index.chunk_ids()` vs. its own `records: DashMap<ChunkId, EmbeddingRecord>`:
- No record for a `ChunkId` → **new chunk**, batched into one `provider.embed(...)` call.
- Record exists but `record.chunk_hash != chunk.metadata.hash` → **changed**, re-embedded in the same batch (chunk `hash` already encodes "did this span's content or position change" from the Chunk Index stage — `sync` doesn't need its own staleness logic).
- A `ChunkId` in `records` no longer present in `chunk_index.chunk_ids()` → **deleted**, removed from both `records` and the vector store.

`EmbeddingIndexSlot` (`Mutex<Option<(usize, EmbeddingIndex)>>`, `commands/embeddings.rs`) is the first `.manage()`'d state in this file that's lazily (re)built rather than constructed once at startup — `usearch`'s index needs a fixed dimension count, and switching provider (Local↔Remote, or a different remote model) can change that dimension, so the slot rebuilds only when the resolved provider's `dimensions()` no longer matches what's currently held.

**IPC surface** (`commands/embeddings.rs`, 7 commands): `embedding_get_config`/`embedding_set_config`, `embedding_set_remote_api_key`/`embedding_has_remote_api_key` (write-only key, mirrors `git_save_credentials`), `embedding_model_status`, `embedding_download_model` (async, streams progress events), `embedding_sync` (async — rebuilds `RepositoryIndex` + `ChunkIndex` for the current project, then `EmbeddingIndex::sync`, returns `SyncStats{embedded, skipped_unchanged, removed}`). Frontend wrappers in `src/lib/embeddings.ts`, shared state in `src/hooks/useEmbeddingSetup.ts`.

**This is the first layer in this file with real UI**: a "Эмбеддинги" tab in Settings (provider choice, model download with a live progress bar, remote base URL/model/API key, manual sync trigger) and a setup checklist in the RightDock "Ассистент" panel (`AssistantPanel.tsx`, replacing what had been an empty stub since the very first AI-harness stage) — "Настроить провайдера" / "Загрузить модель" (Local only) / "Синхронизировать индекс", each item clickable, both consuming the same `useEmbeddingSetup` hook so they never disagree about current state.

## What is not built yet

- No UI to view or change `ai_access_mode` / `ai_allowed_tools` — currently editable only by hand-editing `project.json` or in Rust code/tests.
- No LLM client, no chat UI, no streaming — this file only governs *what a future harness may read*, not how it talks to a model.
- No write/edit tool.
- No logging at the `execute_tool` call site (the single entry point exists specifically so this is a one-line addition later).
- No frontend caller of `aiExecuteTool` yet — the IPC boundary is wired and verified, but nothing in the UI triggers it.
- Neither Repository Index nor Chunk Index has a `#[tauri::command]` of its own or any wiring into `ai_tools`'s `ToolCall`/`ToolName` — both are only reachable indirectly, via `embedding_sync` rebuilding them internally.
- No persistence to disk for Repository Index, Chunk Index, or the embedding vectors — `embedding_sync` rebuilds `RepositoryIndex`/`ChunkIndex` from scratch every call, and `EmbeddingIndex`/`VectorStore` live only in memory for the app session (restart loses everything and the next sync re-embeds unchanged chunks from scratch, since there's no cached `chunk_hash` to diff against).
- No actual file-watcher-driven incremental rebuild — `embedding_sync` (and, underneath it, `ChunkBuilder::build_file` + `ChunkIndex::replace_for_file`) exist in the shape a watcher will call, but nothing triggers a sync on a file-change event yet; it only runs when a user clicks "Синхронизировать" in Settings or the Assistant panel.
- No semantic-search IPC command or UI — `EmbeddingIndex::search` exists and is unit-tested, but nothing calls it; this stage only builds the index, not a way to query it.
- No `ai_tools`/`ToolCall` integration for embeddings — a future `SemanticSearch` tool would sit in `services/ai_tools.rs` next to `ReadFile`/`ListFiles`, but doesn't exist yet.
- Model download progress is coarse (0%/100% only) — see "Embeddings" above for why.

## Extending this

Adding a new tool touches exactly these spots:
1. `domain/ai_access.rs`: add the `ToolName` variant, decide whether it belongs in `default_allowed_tools`.
2. `domain/ai_tools.rs`: add args/result types if needed, a `ToolCall`/`ToolResult` variant, update `ToolCall::name()`.
3. `services/ai_tools.rs`: implement the tool as a private function, add one match arm in `execute_tool`.
4. If the tool can read content outside `ensure_under`'s reach (like the git-diff case above), do **not** add it to `default_allowed_tools` — require explicit per-project opt-in.
