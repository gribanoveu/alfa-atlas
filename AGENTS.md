# AGENTS.md

Context for AI coding agents (Claude Code and others) working in this repository.

## Project

**Alfa Atlas** — a documentation editor that works directly with Git repositories.

- Identifier: `com.eugene.alfa-atlas`
- Stack: Tauri v2, React + TypeScript frontend, Rust backend
- Package manager: **bun** — always use `bun`/`bunx`, never `npm`/`pnpm`/`yarn`

## Setup & commands

```bash
bun install                 # install frontend deps
bun run tauri dev           # run the app with hot reload
bun run tauri build         # production bundle
bun run tsc --noEmit        # type-check frontend only
cd src-tauri && cargo check # fast Rust check while iterating
cd src-tauri && cargo test  # run Rust tests
cd src-tauri && cargo add <crate>   # add a Rust dependency
bun add <pkg>                       # add a JS dependency
```

Run `bun run tsc --noEmit` and `cargo check` before considering a change done. If tests exist for the touched area, run them too — don't assume green.

## Architecture

The app is layered; keep new code in the right layer instead of adding logic to the boundary.

```
src/                      # React frontend
├── components/           # render only — no fetching, no business rules
├── hooks/                 # frontend application layer: call invoke() wrappers, hold state
└── lib/                    # boundary: one typed function per Tauri command, no logic

src-tauri/src/
├── commands/              # boundary: thin #[tauri::command] fns — validate input, call a service, map errors to String
├── services/               # application layer: use-case orchestration
├── domain/                  # pure types + business rules + typed error enums (thiserror), no I/O, no framework types
└── infra/                    # git2 / filesystem / network — concrete implementations of domain traits
```

Dependency direction points inward: `commands → services → domain`, and `infra` implements traits that `domain`/`services` define — never the reverse. Don't reach for `git2` or `tauri::` types from inside `domain/`.

Don't pre-build all four layers for something trivial. Introduce a trait boundary when there's a real second implementation (e.g. a test double) or a use-case spanning multiple infra calls — not speculatively.

See [`AI_HARNESS.md`](AI_HARNESS.md) for the AI-agent tool-access infrastructure (`domain/ai_access.rs`, `domain/ai_tools.rs`, `services/ai_tools.rs`) — backend-only scaffolding, not yet wired to any LLM or UI.

## Errors

- Model failures as data as deep into the stack as possible: `thiserror` enums in `domain/`, not `String`.
- Flatten to `String` (or a small serializable DTO) only inside `commands/`, at the IPC boundary — that's the one place stringly-typed errors are acceptable.
- No `unwrap()`/`expect()` outside of tests and truly-unreachable invariants.

## IPC conventions

- Every `#[tauri::command]` gets a matching typed wrapper in `src/lib/` — components/hooks call the wrapper, never `invoke()` directly.
- New commands must be registered in `generate_handler![]` in `main.rs`, and any new plugin/API surface needs a corresponding entry in `src-tauri/capabilities/*.json` — a command that "does nothing" at runtime usually means a missing capability entry, not a missing registration.
- Long-running git operations (clone, fetch) run as `async` commands or via `spawn_blocking`, not on the IPC event loop.

## Filesystem & git

- Prefer `git2` over shelling out to `git` for programmatic operations (diff, blame, branch listing); shell out only for actions you don't want to reimplement, isolated to one module.
- Validate/canonicalize any path coming from the frontend against the opened repo root before touching it.
- Use `std::path::Path`/`PathBuf` throughout — no manual path string concatenation (the app targets macOS/Windows/Linux).

## Style

- Prefer immutable data and explicit, exhaustive error handling (`match` over ignoring `Err`).
- Keep abstractions proportionate to actual need — don't add a layer, trait, or generic parameter for a hypothetical future case.
- Match existing patterns in the file/module you're editing before introducing a new one.

## Commit / PR expectations

- Keep commits scoped to one layer or one concern where practical.
- Run the relevant checks above before marking work done; mention in the summary which checks were run.
