export type JsonValue = Record<string, unknown>;

export type OperationSummary = {
  path: string;
  method: string;
  operationId?: string;
  summary?: string;
  tags: string[];
};

export type RefMarker = {
  $ref: string;
  unresolved?: boolean;
  circular?: boolean;
  reason?: string;
};

const HTTP_METHODS = [
  "get",
  "put",
  "post",
  "delete",
  "options",
  "head",
  "patch",
  "trace",
];

export function asObject(value: unknown): JsonValue | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as JsonValue)
    : null;
}

export function collectOperations(document: JsonValue): OperationSummary[] {
  const paths = asObject(document.paths);
  if (!paths) return [];
  const result: OperationSummary[] = [];
  for (const [path, pathItemRaw] of Object.entries(paths)) {
    const pathItem = asObject(pathItemRaw);
    if (!pathItem) continue;
    for (const method of HTTP_METHODS) {
      const op = asObject(pathItem[method]);
      if (!op) continue;
      result.push({
        path,
        method,
        operationId: typeof op.operationId === "string" ? op.operationId : undefined,
        summary: typeof op.summary === "string" ? op.summary : undefined,
        tags: Array.isArray(op.tags)
          ? op.tags.filter((t): t is string => typeof t === "string")
          : [],
      });
    }
  }
  return result;
}

const OTHER_TAG = "Other";

export function groupByTag(
  operations: OperationSummary[],
): Map<string, OperationSummary[]> {
  const groups = new Map<string, OperationSummary[]>();
  for (const op of operations) {
    const tags = op.tags.length > 0 ? op.tags : [OTHER_TAG];
    for (const tag of tags) {
      const list = groups.get(tag) ?? [];
      list.push(op);
      groups.set(tag, list);
    }
  }
  return groups;
}

export function getOperation(
  document: JsonValue,
  path: string,
  method: string,
): JsonValue | null {
  const paths = asObject(document.paths);
  if (!paths) return null;
  const pathItem = asObject(paths[path]);
  if (!pathItem) return null;
  return asObject(pathItem[method]);
}

export function isRefMarker(value: unknown): value is RefMarker {
  const obj = asObject(value);
  if (!obj || typeof obj.$ref !== "string") return false;
  return Boolean(obj.unresolved || obj.circular);
}

export type ParamEntry = {
  name: string;
  in: string;
  required: boolean;
  description: string | null;
  schema: unknown;
};

/** Parses `operation.parameters` into a flat list, skipping unresolved refs
 * (they'd otherwise render as garbage `{$ref:...}` entries). Shared by the
 * read-only parameters table and the "Try it out" form. */
export function parseParameters(operation: JsonValue): ParamEntry[] {
  if (!Array.isArray(operation.parameters)) return [];
  return operation.parameters
    .map((p) => asObject(p))
    .filter((p): p is JsonValue => p !== null && !isRefMarker(p))
    .map((p) => ({
      name: typeof p.name === "string" ? p.name : "?",
      in: typeof p.in === "string" ? p.in : "?",
      required: Boolean(p.required),
      description: typeof p.description === "string" ? p.description : null,
      schema: p.schema,
    }));
}

/** First `application/json`-ish media type entry of `requestBody.content`,
 * or the first entry of any type if none is JSON. */
export function primaryRequestBodyMedia(
  operation: JsonValue,
): { mediaType: string; schema: unknown } | null {
  const requestBody = asObject(operation.requestBody);
  const content = requestBody ? asObject(requestBody.content) : null;
  if (!content) return null;
  const entries = Object.entries(content);
  const jsonEntry = entries.find(([mt]) => mt.includes("json")) ?? entries[0];
  if (!jsonEntry) return null;
  const [mediaType, mediaObj] = jsonEntry;
  return { mediaType, schema: asObject(mediaObj)?.schema };
}

export function matchesFilter(op: OperationSummary, query: string): boolean {
  if (!query) return true;
  const q = query.toLowerCase();
  return (
    op.path.toLowerCase().includes(q) ||
    op.method.toLowerCase().includes(q) ||
    (op.operationId?.toLowerCase().includes(q) ?? false) ||
    (op.summary?.toLowerCase().includes(q) ?? false) ||
    op.tags.some((t) => t.toLowerCase().includes(q))
  );
}
