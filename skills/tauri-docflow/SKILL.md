---
name: tauri-docflow-dev
description: Working with the docflow desktop app (Tauri v2 + React/TypeScript + bun). Use this whenever the user is developing, debugging, building, or extending docflow — adding Tauri commands, wiring frontend↔backend IPC, touching src-tauri/ (Rust) or src/ (React/TS), changing permissions/capabilities, working with the filesystem or git operations from Rust, or running dev/build scripts. Also trigger for general Tauri v2 questions asked in the context of this project.
---

# docflow — Tauri dev skill

## Project identity

- Name: **Alfa Atlas**
- Identifier: `com.eugene.alfa-atlas`
- Frontend: TypeScript + React, built and run with **bun** (not npm/pnpm/yarn — always use `bun`/`bunx`)
- Backend: Rust (Tauri v2)
- Purpose: a documentation editor that works directly with Git repositories (see project notes for scope)

## Project layout

```
alfa-atlas/
├── src/                   # React frontend
│   ├── components/
│   ├── hooks/
│   ├── lib/               # typed wrappers around invoke() — see "IPC" below
│   └── main.tsx
├── src-tauri/
│   ├── src/
│   │   ├── main.rs        # entrypoint, builds the Tauri app, registers commands
│   │   ├── commands/       # thin #[tauri::command] fns, one module per feature area
│   │   ├── ...             # domain/service/repo layers — see the layering skill
│   ├── capabilities/       # Tauri v2 permission manifests (per-window ACL)
│   ├── tauri.conf.json
│   └── Cargo.toml
├── package.json
└── bun.lockb
```

## Everyday commands

- Install deps: `bun install`
- Run dev (hot reload, opens the native window): `bun run tauri dev`
- Type-check frontend only: `bun run tsc --noEmit`
- Build release bundle: `bun run tauri build`
- Rust-only check (faster than a full tauri build while iterating on commands): `cd src-tauri && cargo check`
- Add a Rust dependency: `cd src-tauri && cargo add <crate>`
- Add a JS dependency: `bun add <pkg>` (or `bun add -d <pkg>` for dev deps)

Always prefer `bun run tauri ...` over calling the global `tauri` CLI directly, so the project-local Tauri CLI version from `package.json` is used.

## IPC pattern (Rust ⇄ React)

Every backend capability is exposed as a `#[tauri::command]`. Keep commands **thin** — they should parse input, call into the service/domain layer, and map the result/error into something serializable. Business logic does not belong in the command function itself (see the layering skill for why).

Rust side:

```rust
// src-tauri/src/commands/repo.rs
#[tauri::command]
pub async fn open_repository(path: String) -> Result<RepoSummary, String> {
    repo_service::open(&path)
        .map_err(|e| e.to_string()) // domain errors -> string at the IPC boundary only
}
```

Register it in `main.rs`:

```rust
.invoke_handler(tauri::generate_handler![
    commands::repo::open_repository,
    // ...
])
```

Frontend side — never call `invoke()` ad hoc from components. Wrap each command once in `src/lib/`:

```ts
// src/lib/repo.ts
import { invoke } from '@tauri-apps/api/core';

export interface RepoSummary { /* mirror the Rust struct */ }

export function openRepository(path: string): Promise<RepoSummary> {
  return invoke('open_repository', { path });
}
```

This keeps the string command names and payload shapes in exactly one place per side, and gives components a typed, mockable API to call.

### Error handling across the boundary

- `Result<T, String>` is the pragmatic default for `#[tauri::command]` returns, since Tauri needs the error to be serializable.
- Don't just `.to_string()` a raw error at random points — define a domain error enum (`thiserror`) in the service/domain layer, and convert to `String` *only* in the command function, right at the IPC edge. That keeps the internal Rust code using proper typed errors while the boundary stays JSON-friendly.
- On the frontend, `invoke()` rejects with that string on `Err` — wrap calls in try/catch and surface a typed `Result`-like shape to the UI layer if you want to avoid throwing across React component boundaries.

## Permissions / capabilities (Tauri v2)

Tauri v2 uses a capability-based ACL instead of the old `allowlist`. Any new API surface (fs access, dialog, shell, etc.) needs an explicit entry in `src-tauri/capabilities/*.json` referencing the window it applies to, e.g.:

```json
{
  "identifier": "main-capability",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "fs:allow-read-text-file",
    "dialog:allow-open"
  ]
}
```

When a new Tauri plugin is added (`cargo add tauri-plugin-fs`, etc.), it also needs registering in `main.rs` via `.plugin(...)` **and** its permissions added to the relevant capability file — forgetting the capability entry is the most common "command exists but silently fails" bug in Tauri v2.

## Git / filesystem access from Rust

Since Alfa Atlas's core feature is reading/writing a Git working tree:

- Prefer the `git2` crate (libgit2 bindings) over shelling out to `git` for anything programmatic (diffing, reading blobs, listing branches) — it's faster, doesn't depend on the user's PATH, and errors are typed.
- Shelling out (`std::process::Command`) is acceptable for actions you don't want to reimplement (e.g. invoking a configured merge/diff tool), but keep it isolated to one module.
- Any filesystem path coming from the frontend should be canonicalized and validated to stay inside the opened repository root before touching it — don't trust raw paths from IPC.
- Long-running git operations (clone, fetch) should be `async` commands or run on a blocking thread pool (`tauri::async_runtime::spawn_blocking`) so they don't stall the IPC event loop.

## Debugging

- Frontend: right-click → Inspect Element in the dev window works like a normal browser devtools (WebView2/WebKit).
- Rust: `RUST_LOG=debug bun run tauri dev` if `tracing`/`env_logger` is wired up; otherwise `dbg!()`/`eprintln!()` show up in the terminal running `tauri dev`.
- rust-analyzer works normally against `src-tauri/Cargo.toml` — treat `src-tauri` as a standalone Rust crate for tooling purposes.

## Common gotchas

- Forgetting to register a new command in `generate_handler![]` → frontend gets a runtime "command not found" error, not a compile error.
- Forgetting the capability/permission entry for a new plugin → command call resolves but silently does nothing or errors at runtime.
- Non-`Send` types held across `.await` in an async command → compile error; keep Rust state behind `Mutex`/`RwLock` wrapped in `tauri::State`, and don't hold the guard across an await point.
- Path separators/case-sensitivity differences across OSes — always go through `std::path::Path`/`PathBuf`, never manual string concatenation, since Alfa Atlas needs to behave on macOS/Windows/Linux alike.
