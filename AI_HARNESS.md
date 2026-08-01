# AI Harness

Status of the AI-agent infrastructure in this app: what exists today, what it does, and what is deliberately not built yet.

## Status

**Backend-only scaffolding. No LLM is wired up, no chat UI exists, no Tauri commands expose any of this to the frontend yet.** This is groundwork for a future AI harness (an agent loop that calls an LLM with tool-calling) — specifically the trust boundary that decides *which files* and *which operations* that harness is allowed to touch. Everything below lives in Rust (`src-tauri/src`), is unit-tested, and is currently unreachable from the app itself.

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
services/ai_tools.rs  — execute_tool(), scope_for_config()                    (orchestration)
infra/workspace_scanner.rs — scan() / scan_all() (gitignore-aware file walk; scan_all skips the doc-format filter, used by FullRepo listing)
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

## What is not built yet

- No `#[tauri::command]`/IPC surface over `execute_tool` — nothing in the frontend can call this today.
- No UI to view or change `ai_access_mode` / `ai_allowed_tools` — currently editable only by hand-editing `project.json` or in Rust code/tests.
- No LLM client, no chat UI, no streaming — this file only governs *what a future harness may read*, not how it talks to a model.
- No write/edit tool.
- No logging at the `execute_tool` call site (the single entry point exists specifically so this is a one-line addition later).

## Extending this

Adding a new tool touches exactly these spots:
1. `domain/ai_access.rs`: add the `ToolName` variant, decide whether it belongs in `default_allowed_tools`.
2. `domain/ai_tools.rs`: add args/result types if needed, a `ToolCall`/`ToolResult` variant, update `ToolCall::name()`.
3. `services/ai_tools.rs`: implement the tool as a private function, add one match arm in `execute_tool`.
4. If the tool can read content outside `ensure_under`'s reach (like the git-diff case above), do **not** add it to `default_allowed_tools` — require explicit per-project opt-in.
