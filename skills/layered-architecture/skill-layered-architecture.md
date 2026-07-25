---
name: layered-app-architecture
description: How to design and split an application into maintainable layers before or while writing code. Use this whenever the user is starting a new app/feature, asks "how should I structure this", is deciding where a piece of logic belongs, is worried about a component or module becoming a god-object, or is refactoring a tangled codebase into cleaner boundaries. Applies to full-stack apps generally, with concrete guidance for a Tauri (React/TS frontend + Rust backend) split like docflow.
---

# Layered architecture design skill

## Core idea

A maintainable app separates **what the app does** (domain/business rules) from **how it's triggered** (UI, IPC, HTTP) and **how it talks to the outside world** (filesystem, git, network, DB). The point isn't ceremony — it's that each layer can change for its own reason without dragging the others with it, and each layer can be tested without the others.

Dependency direction always points inward, toward the domain:

```
UI / IPC layer  →  Application (use-case) layer  →  Domain layer
                                ↑
                     Infrastructure layer (implements interfaces the domain defines)
```

The domain layer never imports from UI, infrastructure, or frameworks. Infrastructure depends on the domain's interfaces (ports), not the other way around — this is the one inversion worth insisting on, because it's what lets you swap git2 for a different backend, or a REST API for local files, without touching business logic.

## The four layers, concretely

1. **Domain** — plain data types and pure logic. No I/O, no framework types, no `async`. In Rust: plain structs/enums, `thiserror` error enums, and functions that take data and return data. This is the layer that should be trivial to unit test with no mocks.
2. **Application / services** — orchestrates domain logic to fulfill a use case ("open a repository", "save a document version"). Talks to infrastructure only through traits/interfaces the domain or application layer defines, never a concrete implementation. This is where transactions, ordering of steps, and error translation between subsystems happen.
3. **Infrastructure** — the actual git2 calls, filesystem access, network clients. Implements the traits the application layer depends on. Free to be messy/imperative since it's isolated.
4. **UI / boundary** — React components + hooks on the frontend; `#[tauri::command]` functions on the backend. Thin. Its only jobs: collect input, call one use-case, render/return the result. If a command function has branching business logic in it, that logic has leaked out of its layer.

## Applying this to a Tauri app (frontend)

- **Components** — render only; no fetching, no business rules.
- **Hooks** (`useX`) — application layer for the frontend: call the typed `invoke()` wrappers, hold local state/loading/error, expose a clean interface to components.
- **lib/ (IPC wrappers)** — boundary layer: one function per Tauri command, typed request/response, no logic beyond mapping.
- Domain concepts that exist on both sides (e.g. a `Document`, a `RepoSummary`) should have matching, hand-kept-in-sync types on each side — don't try to auto-share Rust types into TS unless the project already has codegen tooling for it; for a project this size, duplicating a small interface is cheaper than the tooling.

## Applying this to a Tauri app (backend)

```
src-tauri/src/
├── commands/     # boundary: thin #[tauri::command] fns
├── services/      # application: use-cases, orchestration
├── domain/        # pure types + business rules + error enums
├── infra/         # git2 wrappers, fs access, concrete implementations
└── main.rs
```

- `domain/` defines traits like `trait DocumentRepository { fn read(&self, path: &Path) -> Result<Document, DomainError>; }`
- `infra/` provides `struct GitDocumentRepository` implementing that trait using `git2`.
- `services/` takes a `&dyn DocumentRepository` (or a generic bound) and implements the use-case, independent of git2 specifically.
- `commands/` constructs/receives the concrete infra implementation (often via `tauri::State`) and calls the service.

This means: swapping git2 for shelling out to the `git` binary, or adding a test double that reads from an in-memory map, only touches `infra/` — the domain and service logic, and their tests, don't change.

## Error modeling across layers

Consistent with an explicit-error-modeling style: model failures as data, not exceptions/strings, as deep into the stack as possible.

- `domain/` — a `thiserror` enum per bounded concept (`DocumentError`, `RepoError`), no `String` errors.
- `services/` — either reuses the domain error or defines a composed enum (`#[from]` conversions) when a use-case can fail for reasons from multiple domains.
- `commands/` — the *only* place that flattens a typed error down to `String` (or a small serializable DTO) for the IPC boundary, per the docflow Tauri skill.

This gives you `match`-able, exhaustive error handling everywhere except the one unavoidable seam (crossing process/IPC boundaries), where you deliberately give it up.

## Sizing the abstraction to the app

Don't build all four layers on day one for a five-command prototype — that's over-engineering a thing that doesn't need it yet. A reasonable progression:

1. **Prototype stage**: commands call infra directly, no traits. Fine while there's one implementation and no tests to speak of.
2. **First trait boundary**: introduce it the moment you have *either* (a) a second implementation you actually need (e.g. a mock for tests), or (b) a use-case that spans more than one infra call and needs its own orchestration/error handling. That's your service layer being born — don't pre-build it speculatively.
3. **Full domain separation**: worth it once business rules (validation, merge/versioning logic, etc.) get complex enough that you want to unit-test them without spinning up git2 or a Tauri runtime at all.

The signal to add a layer is a concrete pain (untestable logic, duplicated orchestration, a second backend you need to support) — not a rule that says "always have 4 layers."

## Smells that mean a boundary is missing

- A React component directly calling `invoke()` with inline error handling repeated in multiple places → needs an `lib/` wrapper + hook.
- A `#[tauri::command]` function with `if`/`match` business logic beyond input validation → needs a service function to delegate to.
- Domain logic reaching for `tauri::` types, or importing `git2` directly in code you want to unit test → needs a trait boundary between domain/service and infra.
- The same "shape" of error handling copy-pasted in several commands → extract into the service layer or a shared error-conversion helper at the boundary.
