export type JsonValue = Record<string, unknown>;

export type OperationSummary = {
  path: string;
  method: string;
  operationId?: string;
  summary?: string;
  tags: string[];
  deprecated: boolean;
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
        deprecated: op.deprecated === true,
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

export function getPathItem(document: JsonValue, path: string): JsonValue | null {
  const paths = asObject(document.paths);
  if (!paths) return null;
  return asObject(paths[path]);
}

export function getOperation(
  document: JsonValue,
  path: string,
  method: string,
): JsonValue | null {
  const pathItem = getPathItem(document, path);
  if (!pathItem) return null;
  return asObject(pathItem[method]);
}

export type ServerEntry = { url: string; description: string | null };

/** Спека разрешает переопределять `servers` на уровне path item и отдельной
 * операции; побеждает самый узкий уровень. Без этого «Try it out» отправлял
 * бы запрос на корневой хост даже там, где ручка живёт на другом. */
export function effectiveServers(
  document: JsonValue,
  path: string,
  operation: JsonValue | null,
): ServerEntry[] {
  const pathItem = getPathItem(document, path);
  const raw =
    (operation && Array.isArray(operation.servers) && operation.servers) ||
    (pathItem && Array.isArray(pathItem.servers) && pathItem.servers) ||
    (Array.isArray(document.servers) ? document.servers : []);
  return (raw as unknown[])
    .map((entry) => asObject(entry))
    .filter((entry): entry is JsonValue => entry !== null)
    .map((entry) => ({
      url: typeof entry.url === "string" ? entry.url : "",
      description: typeof entry.description === "string" ? entry.description : null,
    }))
    .filter((entry) => entry.url !== "");
}

const LOCAL_SCHEMA_REF_PREFIX = "#/components/schemas/";

/** Имя схемы, на которую ведёт внутренняя ссылка вида
 * `#/components/schemas/Node`. Такие ссылки оставляет сборщик на месте
 * рекурсии (`Node.children: [Node]`): развернуть её нельзя, а вынесенная в
 * `components/schemas` схема — валидный OpenAPI, который понимают и
 * генераторы, и вьюер. `null` — это не ссылка на компонент. */
export function localSchemaRefName(value: unknown): string | null {
  const obj = asObject(value);
  if (!obj || typeof obj.$ref !== "string") return null;
  if (!obj.$ref.startsWith(LOCAL_SCHEMA_REF_PREFIX)) return null;
  const name = obj.$ref.slice(LOCAL_SCHEMA_REF_PREFIX.length);
  return name === "" ? null : name;
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

/** Параметры операции вместе с общими для всего path item. Спека объявляет
 * общие параметры (обычно `{id}` пути) один раз на все методы ручки; без
 * слияния они пропадали и из таблицы, и из формы запроса, а валидация ругалась
 * на «необъявленный path-параметр». Совпадение по (name, in) выигрывает за
 * операцией — она уточняет унаследованное. */
export function effectiveParameters(
  document: JsonValue,
  path: string,
  operation: JsonValue,
): ParamEntry[] {
  const pathItem = getPathItem(document, path);
  const shared = pathItem ? parseParameters(pathItem) : [];
  const own = parseParameters(operation);
  const ownKeys = new Set(own.map((p) => `${p.in}:${p.name}`));
  return [...shared.filter((p) => !ownKeys.has(`${p.in}:${p.name}`)), ...own];
}

export type NamedExample = {
  name: string;
  summary: string | null;
  description: string | null;
  value: unknown;
};

/** Именованные примеры media type (`examples: { ok: { value: … } }`) — вьюер
 * раньше показывал только одиночный `example`, хотя в реальных спеках именно
 * `examples` описывают интересные случаи (пустой список, ошибка бизнес-логики). */
export function namedExamples(media: JsonValue): NamedExample[] {
  const examples = asObject(media.examples);
  if (!examples) return [];
  return Object.entries(examples)
    .map(([name, raw]) => {
      const entry = asObject(raw);
      if (!entry || isRefMarker(entry)) return null;
      return {
        name,
        summary: typeof entry.summary === "string" ? entry.summary : null,
        description: typeof entry.description === "string" ? entry.description : null,
        value: entry.value,
      };
    })
    .filter((entry): entry is NamedExample => entry !== null && entry.value !== undefined);
}

export type ExternalDocs = { url: string; description: string | null };

export function externalDocsOf(node: JsonValue): ExternalDocs | null {
  const docs = asObject(node.externalDocs);
  if (!docs || typeof docs.url !== "string" || docs.url === "") return null;
  return {
    url: docs.url,
    description: typeof docs.description === "string" ? docs.description : null,
  };
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
