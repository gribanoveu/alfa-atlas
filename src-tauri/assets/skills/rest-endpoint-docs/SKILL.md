---
name: rest-endpoint-docs
description: Fill a REST API method documentation folder after its scaffold exists. Use when documenting a new REST method, completing createDirectory restEndpoint files, writing algorithm/validation/sequence in a method folder, or the user asks for method documentation (документация метода, алгоритм, валидация).
---

# REST method documentation (after scaffold)

The folder already exists (UI «Документация на REST метод» or `createDirectory` with `template: "restEndpoint"`). This skill is how to **fill** those files, not how to create the folder.

## Folder convention

One folder = one method. Do not rename scaffold files.

- `{methodName}.adoc` — main method document (`methodName` = final path segment)
- `request.adoc` — request example (bare name, never `{methodName}-request.adoc`)
- `response.adoc` — response example (bare name)
- `{methodName}.puml` — sequence/activity diagram (PlantUML)

## Main file `{methodName}.adoc`

Keep this section order. Replace placeholders; do not invent a different outline.

1. Document header (`= …`, `:sectnums:`, `:toc:`)
2. **Назначение** — short purpose (may match an index blurb)
3. **Описание входных/выходных параметров**
   - **Входные параметры** — five-column table: Параметр, Формат, Обязательный, Описание, Варианты значений. Use `-` for an empty variants cell.
   - **Пример запроса** — `include::request.adoc[]` only; do not paste the curl/JSON into the main file
   - **Выходные параметры** — same table shape; add mapping notes when a field comes from an external service
   - **Пример ответа** — `include::response.adoc[]`
4. **Схема работы** — PlantUML include of the `.puml` in the same folder:

```
[plantuml, "{methodName}", png]
----
include::{methodName}.puml[]
----
```

(The scaffold may still say `sequence_diagramm` in comments; the generated include target is `{methodName}.puml`.)

5. **Алгоритм работы** — numbered list. **First item is always «Валидация входных параметров».** Later headings repeat those list items.
6. **Валидация входных параметров** — table Параметр / Условие / Результат. Cover parameters that are actually validated; if the set is small, list the rest as «не валидируется/не проверяется». All columns required. Use row-span (`.2+|`) when one parameter has several conditions.
7. Per-step sections for business/external calls:
   - Heading must include a `link:` to the used service's docs in git, e.g. `=== Шаг 1. Вызов link:ссылка[названиеМетода]`
   - Optional one-line why the service is used, if the title is not enough
   - Nested input/output tables (same five columns)
   - Examples via include from `../_external/{service}/{method}-request.adoc` and `…-response.adoc` — do not inline those payloads
8. **Формирование ответа** — mapping table Параметр / Правило заполнения when the response is assembled from external calls
9. **Обработка ошибок** — Код / Описание table for this method's errors, plus `include::../CompositeException.adoc[]` when the shared CompositeException description applies

Prose is Russian; identifiers, paths, JSON keys, enum values stay English.

## `request.adoc` / `response.adoc`

Keep the collapsible example wrapper from the scaffold (`<details>` / AsciiDoc open block). Fill path params, body params, a real curl/JSON example, and possible errors in `request.adoc`. Put a realistic JSON success (and error if useful) in `response.adoc`. Do not duplicate those examples in `{methodName}.adoc` beyond the `include::`.

## What not to do

- Do not rename `request.adoc` / `response.adoc` to match a legacy `{method}-request.adoc` pattern
- Do not skip validation as the first algorithm step
- Do not invent a different house outline; edit placeholders in place
- Creating the folder is a separate tool call (`createDirectory` / the New folder dialog) — this skill starts after that
