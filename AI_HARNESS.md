# AI Harness

Status of the AI-agent infrastructure in this app: what exists today, what it does, and what is deliberately not built yet.

## Status

**The tool-execution boundary is reachable from the frontend via one IPC command (`ai_execute_tool`), and a standalone structural Repository Index now exists alongside it — but no LLM is wired up and no chat UI exists yet.** This is groundwork for a future AI harness (an agent loop that calls an LLM with tool-calling) — specifically the trust boundary that decides *which files* and *which operations* that harness is allowed to touch, plus a per-file index of metadata/hash/language/symbols it will eventually query. The Rust side (`src-tauri/src`) is unit-tested throughout; the IPC command is additionally verified end-to-end (see "IPC surface" below) but has no caller anywhere in the app yet, and the Repository Index has no caller at all yet (not even IPC) — nothing in `src/components`/`src/hooks` invokes either.

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
services/repo_index.rs      — RepositoryIndex (build/get/files_for_language/clear)
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

## What is not built yet

- No UI to view or change `ai_access_mode` / `ai_allowed_tools` — currently editable only by hand-editing `project.json` or in Rust code/tests.
- No LLM client, no chat UI, no streaming — this file only governs *what a future harness may read*, not how it talks to a model.
- No write/edit tool.
- No logging at the `execute_tool` call site (the single entry point exists specifically so this is a one-line addition later).
- No frontend caller of `aiExecuteTool` yet — the IPC boundary is wired and verified, but nothing in the UI triggers it.
- Repository Index has no `#[tauri::command]`, no `.manage()` registration, no wiring into `ai_tools`'s `ToolCall`/`ToolName`, no persistence to disk, and no chunking (`Chunk`/`ChunkStrategy` — a deliberately separate future stage once the index exists to build chunks from).

## Extending this

Adding a new tool touches exactly these spots:
1. `domain/ai_access.rs`: add the `ToolName` variant, decide whether it belongs in `default_allowed_tools`.
2. `domain/ai_tools.rs`: add args/result types if needed, a `ToolCall`/`ToolResult` variant, update `ToolCall::name()`.
3. `services/ai_tools.rs`: implement the tool as a private function, add one match arm in `execute_tool`.
4. If the tool can read content outside `ensure_under`'s reach (like the git-diff case above), do **not** add it to `default_allowed_tools` — require explicit per-project opt-in.
