---
name: openapi-specs-layout
description: OpenAPI multi-file spec layout used by Atlas (specs root with schemas, responses, parameters, operations, and $ref). Use when the user mentions OpenAPI, swagger, schemas, operations, $ref, or Project type is OpenAPI Specification. Русские формы — спецификация, спецификации, спецификацию, спеку openapi, схемы openapi, операции openapi.
---

# OpenAPI multi-file spec layout

Atlas detects a spec root the same way the API Explorer does: a directory with an entry YAML/JSON file whose top-level key is `openapi:` or `swagger:`, plus any of the structural subfolders below. None of the subfolders is mandatory on its own.

## Root

The documentation root **is** the spec root (often named `specs/` at the repo root, but the detected docs root is what tools use).

- Entry file sits **directly** in that root (not inside `operations/`): YAML/JSON with `openapi:` / `swagger:`.
- Typical `info.title` / `info.version` are what the UI shows as project type.

## Folders

| Folder | Put here |
| --- | --- |
| `schemas/` | Reusable data models / components |
| `responses/` | Reusable response objects |
| `parameters/` | Reusable parameters |
| `operations/` | One file per operation (path item / operation) |

A real spec may omit a folder (e.g. no `parameters/` if nothing extra is shared). Do not invent a fifth structural folder for those four kinds of objects.

## Where to put a change

- New or edited **schema** → file under `schemas/`. Do not embed a large schema in the operation file when it is shared or would be reused.
- New **operation** → new file under `operations/`. Do not mix a schema definition and a path item in the same file.
- Shared **response** / **parameter** → `responses/` / `parameters/`, then `$ref` from the operation.

## `$ref`

Use **relative** refs from the file that points:

- `./schemas/foo.yaml#/Foo`
- `./operations/getUser.yaml`
- Same-file pointer: `#/taxId`

Do not:

- Copy-paste a definition into several files instead of a `$ref`
- Use absolute filesystem paths or repo-root paths that tools cannot resolve
- Point at a missing file; if the target is new, create it first

## Tool paths

The spec directory is the documentation root.

- **Docs-only:** paths are relative to that root (`schemas/api.yaml`, `operations/getUser.yaml`)
- **Full-repo:** the same files include the docs-root prefix (`specs/schemas/api.yaml` when docs root is `specs`)

Pass paths between tools unchanged. Writes/edits still only succeed under the documentation tree.

## Workflow

1. Confirm the spec root (`listFiles` on `.` / the docs root) — look for the entry file and the four folders.
2. Find the operation or schema with `grep` (exact tokens: an operationId, a schema name, a `$ref` target) or `semanticSearch` (when you only know what the endpoint does), then `readFile` before editing.
3. Add or edit the file in the folder that matches its kind; wire it with a relative `$ref`.
4. Do not flatten the layout into a single mega-YAML unless the repository already uses that style (this skill is for the multi-file convention Atlas detects).
