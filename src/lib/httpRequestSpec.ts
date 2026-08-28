import type { ArtifactContent, HttpRequestSpec, ParamSpec } from "./artifacts";

/** Pure helpers for the HTTP-request artifact builder. Rendering to AsciiDoc
 *  deliberately lives in Rust (`domain::artifact_render`) so the preview and
 *  the assistant's copy cannot drift; what is here is only what the form
 *  itself needs. */

export const HTTP_METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] as const;

export function emptyParam(): ParamSpec {
  return { name: "", format: "string", required: true, description: "", values: "" };
}

/** JSON type name for the «Формат» column. Arrays report their element type
 *  (`array<string>`) since that is what a reader needs; an empty or mixed
 *  array degrades to plain `array`. */
export function jsonFormatOf(value: unknown): string {
  if (value === null) return "null";
  if (Array.isArray(value)) {
    const elementTypes = new Set(value.map((v) => jsonFormatOf(v)));
    return elementTypes.size === 1 ? `array<${[...elementTypes][0]}>` : "array";
  }
  switch (typeof value) {
    case "string":
      return "string";
    case "number":
      return Number.isInteger(value) ? "integer" : "number";
    case "boolean":
      return "boolean";
    case "object":
      return "object";
    default:
      return "string";
  }
}

/** Example value for the «Варианты значений» column. Objects and arrays
 *  contribute nothing — their fields are rows of their own. */
export function jsonExampleOf(value: unknown): string {
  if (value === null || typeof value === "object") return "";
  return String(value);
}

const MAX_INFERRED_DEPTH = 4;
const MAX_INFERRED_PARAMS = 200;

/** Walks a JSON example and produces one parameter row per field, nested
 *  fields dotted (`userData.id`) the way the house templates write them.
 *
 *  Arrays contribute their *element's* fields under the array's own name
 *  rather than an indexed path — `items[0].id` is an artifact of the
 *  example, `items.id` is the documented field. Only the first element is
 *  walked, since a well-formed example's elements share a shape.
 *
 *  Descriptions are left empty on purpose: this saves the user typing
 *  names, types and examples, but what a field *means* is the part only
 *  they know, and pre-filling it with a guess invites it being left as-is. */
export function inferParamsFromJson(sample: string): ParamSpec[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(sample);
  } catch {
    return [];
  }
  const rows: ParamSpec[] = [];
  walk(parsed, "", 0, rows);
  return rows.slice(0, MAX_INFERRED_PARAMS);
}

function walk(value: unknown, prefix: string, depth: number, rows: ParamSpec[]): void {
  if (depth > MAX_INFERRED_DEPTH || rows.length >= MAX_INFERRED_PARAMS) return;

  // A top-level array documents its element's fields, not the array itself.
  if (Array.isArray(value)) {
    if (value.length > 0) walk(value[0], prefix, depth, rows);
    return;
  }
  if (value === null || typeof value !== "object") return;

  for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
    if (rows.length >= MAX_INFERRED_PARAMS) return;
    const name = prefix ? `${prefix}.${key}` : key;
    rows.push({
      name,
      format: jsonFormatOf(child),
      required: true,
      description: "",
      values: jsonExampleOf(child),
    });
    if (Array.isArray(child)) {
      if (child.length > 0) walk(child[0], name, depth + 1, rows);
    } else if (child !== null && typeof child === "object") {
      walk(child, name, depth + 1, rows);
    }
  }
}

/** Merges inferred rows into rows the user already has: an existing row
 *  keeps everything the user typed and only picks up a format/example it
 *  was missing, and rows that are no longer in the sample are kept rather
 *  than dropped (a documented field absent from one example is normal). */
export function mergeInferredParams(existing: ParamSpec[], inferred: ParamSpec[]): ParamSpec[] {
  const byName = new Map(existing.filter((p) => p.name.trim()).map((p) => [p.name.trim(), p]));
  const merged: ParamSpec[] = [];
  const consumed = new Set<string>();

  for (const row of inferred) {
    const prior = byName.get(row.name);
    if (prior) {
      consumed.add(row.name);
      merged.push({
        ...prior,
        format: prior.format.trim() ? prior.format : row.format,
        values: prior.values.trim() ? prior.values : row.values,
      });
    } else {
      merged.push(row);
    }
  }
  for (const row of existing) {
    const name = row.name.trim();
    // A blank-named row is one the user just added and is about to fill in
    // — dropping it here would delete their work on the way to helping.
    if (name && (consumed.has(name) || inferred.some((i) => i.name === name))) continue;
    merged.push(row);
  }
  return merged;
}

/** Path placeholders (`/api/{organizationId}/documents`) the user has not
 *  yet described — the builder offers to add a row for each. */
export function missingPathParams(spec: HttpRequestSpec): string[] {
  const declared = new Set(spec.pathParams.map((p) => p.name.trim()).filter(Boolean));
  const found = [...spec.path.matchAll(/\{([^{}\s]+)\}/g)].map((m) => m[1]!);
  return [...new Set(found)].filter((name) => !declared.has(name));
}

/** A one-line "what is this" for the tab title and the artifacts list —
 *  mirrors `ArtifactRecord::subtitle` on the Rust side. */
export function describeHttpRequest(spec: HttpRequestSpec): string {
  const method = spec.method.trim().toUpperCase();
  const path = spec.path.trim();
  if (!method && !path) return "";
  if (!method) return path;
  if (!path) return method;
  return `${method} ${path}`;
}

export function contentAsHttpRequest(content: ArtifactContent): HttpRequestSpec {
  return content;
}
