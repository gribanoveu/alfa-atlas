import { asObject, isRefMarker, type JsonValue } from "./openApiModel";

export type ParamValues = Record<string, string>;

export function paramKey(location: string, name: string): string {
  return `${location}:${name}`;
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
  if (s.example !== undefined) return s.example;
  if (Array.isArray(s.examples) && s.examples.length > 0) return s.examples[0];
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
}): BuiltRequest {
  const { baseUrl, path, method, paramValues, paramEntries, bodyMediaType, bodyText, hasBody } =
    options;

  let resolvedPath = path;
  const query: string[] = [];
  const headers: Record<string, string> = {};

  for (const { name, in: location } of paramEntries) {
    const value = paramValues[paramKey(location, name)] ?? "";
    if (location === "path") {
      resolvedPath = resolvedPath.split(`{${name}}`).join(encodeURIComponent(value));
    } else if (location === "query" && value !== "") {
      query.push(`${encodeURIComponent(name)}=${encodeURIComponent(value)}`);
    } else if (location === "header" && value !== "") {
      headers[name] = value;
    }
  }

  let url = joinUrl(baseUrl, resolvedPath);
  if (query.length > 0) url += `?${query.join("&")}`;

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
