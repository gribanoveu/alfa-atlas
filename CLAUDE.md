# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Architecture, layering rules, IPC conventions, error handling and UI rules live in
[`AGENTS.md`](./AGENTS.md) — read it, it is the primary source:

@AGENTS.md

What follows is only what `AGENTS.md` does not cover.

## Tests

Frontend tests run on **Bun's own runner** (not vitest/jest), from `src/__tests__/`:

```bash
bun test                                    # all frontend tests (~135 files)
bun test src/__tests__/useGitWorkflow.test.ts   # one file
bun test -t "conflict"                      # one test by name pattern
```

`bunfig.toml` preloads `src/__tests__/setup/happydom.ts`, which registers the DOM
globals and auto-unmounts after each test. A test that renders components works
only through `bun test` — running the file with `bun run` gives it no DOM.

Rust tests are inline `#[cfg(test)] mod tests` blocks (~150 of them) plus
`services/tests_asciidoc_coordinator.rs`:

```bash
cd src-tauri && cargo test                  # all
cd src-tauri && cargo test git_ops          # by name substring
```

## Checks before done

```bash
bun run tsc --noEmit
cd src-tauri && cargo check
```

CI (`.github/workflows/build-release.yml`) only builds and releases on a tag — it
runs neither `tsc` nor `cargo test`. Nothing catches a broken test but you.

## Dev server

`bun run tauri dev` runs the full app. The Vite dev server alone is registered in
`.claude/launch.json` as `vite-dev` on port 1420 — useful for previewing UI, but
every `invoke()` fails there because there is no Tauri backend.

Port 1420 is `strictPort` — if it is taken, the dev server exits instead of
picking another, and `tauri dev` then fails with a blank window.

## Build notes

- `vite.config.ts` splits `monaco` and `asciidoctor` into manual chunks; both are
  statically imported and otherwise land in the entry chunk, blocking the Welcome
  screen. Mermaid and PlantUML already split themselves via dynamic `import()`.
- Windows builds need Strawberry Perl for the vendored OpenSSL used by `libssh2`
  (see README — the default WinCNG backend hangs on SSH clone with ed25519 keys).
- `scripts/clean-build-cache.sh` reclaims Rust target space; `scripts/build-embedding-model.py`
  prepares the local embedding model.

## Where things are documented

| Path | Contents |
|------|----------|
| `doc/implemented-features.md` | What exists today and how it behaves |
| `doc/business-requirements/` | Target requirements, vision, backlog |
| `doc/releasing.md` | Release process: version bump, tag, CI |
| `AI_HARNESS.md` | The AI-agent tool-access layer (large; grep it rather than reading whole) |
| `METRICS.md` | Metrics/telemetry design |
| `app.config.json` | Version and help links surfaced in the app UI |

Runtime state lives outside the repo: global settings in `~/.docflow/settings.json`,
per-project layout in `{project}/.docflow/layout.json`.
