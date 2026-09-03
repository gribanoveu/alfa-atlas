import { asObject, isRefMarker, type JsonValue } from "./openApiModel";
import type { AppliedCredential } from "./security";

export type ParamValues = Record<string, string>;

export function paramKey(location: string, name: string): string {
  return `${location}:${name}`;
}

type SchemaKind = "string" | "integer" | "number" | "boolean" | "object" | "array" | "null" | "any";

function valueKind(value: unknown): SchemaKind {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  if (typeof value === "number") return Number.isInteger(value) ? "integer" : "number";
  if (typeof value === "string") return "string";
  if (typeof value === "boolean") return "boolean";
  if (typeof value === "object") return "object";
  return "any";
}

function schemaKind(s: JsonValue): SchemaKind | SchemaKind[] {
  if (Array.isArray(s.type)) {
    return s.type.filter((type): type is SchemaKind =>
      typeof type === "string" &&
      ["string", "integer", "number", "boolean", "object", "array", "null"].includes(type),
    );
  }
  if (typeof s.type === "string") return s.type as SchemaKind;
  if (s.properties) return "object";
  if (s.items) return "array";
  return "any";
}

function exampleMatchesSchema(s: JsonValue, example: unknown): boolean {
  const actual = valueKind(example);
  if (actual === "null") return s.nullable === true || schemaKind(s) === "null";
  const declared = schemaKind(s);
  if (Array.isArray(declared)) return declared.includes(actual);
  if (declared === "number") return actual === "number" || actual === "integer";
  return declared === "any" || declared === actual;
}

/** Returns an explicit example only when it has the same JSON shape as the
 * schema. Generated OpenAPI documents occasionally leave a scalar example
 * next to a resolved object `$ref`; using that value would make the example
 * disagree with the contract and hide all of the object's fields. */
export function compatibleExampleForSchema(schema: unknown): unknown | undefined {
  if (isRefMarker(schema)) return undefined;
  const s = asObject(schema);
  if (!s) return undefined;
  const example = s.example !== undefined
    ? s.example
    : Array.isArray(s.examples) && s.examples.length > 0
      ? s.examples[0]
      : undefined;
  return example !== undefined && exampleMatchesSchema(s, example) ? example : undefined;
}

/** Generates a plausible starting value for a schema: its `example`/`default`
 * if declared, otherwise a type-appropriate skeleton (empty string, 0, `{}`
 * with skeleton properties, etc). Safe against cycles because the resolver
 * already replaced real reference cycles with `{circular: true}` markers
 * before this ever sees the schema. */
export function skeletonForSchema(schema: unknown): unknown {
  if (isRefMarker(schema)) return null;
  const s = asObject(schema);
  if (!s) return null;
  const example = compatibleExampleForSchema(s);
  if (example !== undefined) return example;
  if (s.default !== undefined) return s.default;
  if (Array.isArray(s.enum) && s.enum.length > 0) return s.enum[0];

  const type =
    typeof s.type === "string"
      ? s.type
      : s.properties
        ? "object"
        : s.items
          ? "array"
          : undefined;

  switch (type) {
    case "object": {
      const props = asObject(s.properties);
      if (!props) return {};
      const out: Record<string, unknown> = {};
      for (const [key, value] of Object.entries(props)) {
        out[key] = skeletonForSchema(value);
      }
      return out;
    }
    case "array":
      return s.items ? [skeletonForSchema(s.items)] : [];
    case "string":
      return "";
    case "integer":
    case "number":
      return 0;
    case "boolean":
      return false;
    default:
      return null;
  }
}

/** Simple scalar string form for a parameter's default value (path/query/header
 * inputs are plain text fields, not JSON editors). */
export function scalarSkeleton(schema: unknown): string {
  const value = skeletonForSchema(schema);
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  return JSON.stringify(value);
}

export function joinUrl(base: string, path: string): string {
  const trimmedBase = base.replace(/\/+$/, "");
  const normalizedPath = path.startsWith("/") ? path : `/${path}`;
  return trimmedBase + normalizedPath;
}

export type BuiltRequest = {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: string | null;
};

export function buildRequest(options: {
  baseUrl: string;
  path: string;
  method: string;
  paramValues: ParamValues;
  paramEntries: { name: string; in: string }[];
  bodyMediaType: string | null;
  bodyText: string;
  hasBody: boolean;
  /** Подстановка из панели авторизации (`credentialsFor`). Параметр самой
   * операции с тем же именем перекрывает её: он введён здесь и сейчас, а
   * значит намеренно. */
  auth?: AppliedCredential[];
}): BuiltRequest {
  const {
    baseUrl,
    path,
    method,
    paramValues,
    paramEntries,
    bodyMediaType,
    bodyText,
    hasBody,
    auth = [],
  } = options;

  let resolvedPath = path;
  const queryPairs: [string, string][] = [];
  const headers: Record<string, string> = {};

  for (const credential of auth) {
    if (credential.in === "header") headers[credential.name] = credential.value;
  }

  for (const { name, in: location } of paramEntries) {
    const value = paramValues[paramKey(location, name)] ?? "";
    if (location === "path") {
      resolvedPath = resolvedPath.split(`{${name}}`).join(encodeURIComponent(value));
    } else if (location === "query" && value !== "") {
      queryPairs.push([name, value]);
    } else if (location === "header" && value !== "") {
      headers[name] = value;
    }
  }

  for (const credential of auth) {
    if (credential.in !== "query") continue;
    if (queryPairs.some(([name]) => name === credential.name)) continue;
    queryPairs.push([credential.name, credential.value]);
  }

  let url = joinUrl(baseUrl, resolvedPath);
  if (queryPairs.length > 0) {
    const query = queryPairs.map(
      ([name, value]) => `${encodeURIComponent(name)}=${encodeURIComponent(value)}`,
    );
    url += `?${query.join("&")}`;
  }

  if (hasBody && bodyText.trim() !== "") {
    headers["Content-Type"] = bodyMediaType ?? "application/json";
  }

  return {
    method: method.toUpperCase(),
    url,
    headers,
    body: hasBody && bodyText.trim() !== "" ? bodyText : null,
  };
}

function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

export function buildCurl(request: BuiltRequest): string {
  const lines = [`curl -X ${request.method} ${shellQuote(request.url)}`];
  for (const [key, value] of Object.entries(request.headers)) {
    lines.push(`  -H ${shellQuote(`${key}: ${value}`)}`);
  }
  if (request.body) {
    lines.push(`  -d ${shellQuote(request.body)}`);
  }
  return lines.join(" \\\n");
}

export function listServerUrls(document: JsonValue): string[] {
  const servers = Array.isArray(document.servers) ? document.servers : [];
  return servers
    .map((s) => asObject(s)?.url)
    .filter((u): u is string => typeof u === "string" && u.length > 0);
}
