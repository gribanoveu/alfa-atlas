import { parse as parseYaml } from "yaml";
import { extensionOf } from "../../lib/fileExtensions";

export type StructuredValue =
  | string
  | number
  | boolean
  | null
  | StructuredValue[]
  | { [key: string]: StructuredValue };

export type ParseResult =
  | { data: StructuredValue; error: null }
  | { data: null; error: string };

export function isEmptyValue(value: unknown): boolean {
  return (
    value === "" ||
    value === null ||
    value === undefined ||
    (typeof value === "object" &&
      value !== null &&
      !Array.isArray(value) &&
      Object.keys(value).length === 0)
  );
}

export type ValueKind = "string" | "number" | "bool";

export function valueKind(value: unknown): ValueKind {
  if (typeof value === "boolean") return "bool";
  if (typeof value === "number") return "number";
  return "string";
}

export function collectPaths(
  data: unknown,
  path: string,
  acc: Set<string> = new Set(),
): Set<string> {
  if (data !== null && typeof data === "object") {
    acc.add(path);
    const entries = Array.isArray(data)
      ? data.map((value, index) => [index, value] as const)
      : Object.entries(data);
    for (const [key, value] of entries) {
      collectPaths(value, `${path}/${key}`, acc);
    }
  }
  return acc;
}

function formatParseError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

export function parseStructuredData(
  content: string,
  filePath: string | null,
): ParseResult {
  const trimmed = content.trim();
  if (trimmed === "") {
    return { data: null, error: null };
  }

  const ext = filePath ? extensionOf(filePath) : "";
  const isYaml = ext === ".yaml" || ext === ".yml";

  try {
    if (isYaml) {
      return {
        data: parseYaml(content, { strict: false }) as StructuredValue,
        error: null,
      };
    }
    return {
      data: JSON.parse(content) as StructuredValue,
      error: null,
    };
  } catch (error) {
    return { data: null, error: formatParseError(error) };
  }
}

export function countLabel(count: number, isArray: boolean): string {
  if (isArray) {
    if (count === 1) return "1 элемент";
    if (count >= 2 && count <= 4) return `${count} элемента`;
    return `${count} элементов`;
  }
  if (count === 1) return "1 ключ";
  if (count >= 2 && count <= 4) return `${count} ключа`;
  return `${count} ключей`;
}

const MAX_KEY_HINT_LEN = 18;
const MAX_VALUE_HINT_LEN = 24;

function truncateHint(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text;
  return `${text.slice(0, maxLen - 1)}…`;
}

function formatCompactPreview(value: StructuredValue): string {
  if (isEmptyValue(value)) return "null";
  const kind = valueKind(value);
  if (kind === "number" || kind === "bool") {
    return truncateHint(String(value), MAX_VALUE_HINT_LEN);
  }
  if (typeof value === "string") {
    return truncateHint(`"${value}"`, MAX_VALUE_HINT_LEN);
  }
  if (Array.isArray(value)) {
    return value.length === 0 ? "[]" : `[${value.length}]`;
  }
  if (typeof value === "object" && value !== null) {
    const keys = Object.keys(value);
    return keys.length === 0 ? "{}" : truncateHint(keys[0], MAX_VALUE_HINT_LEN);
  }
  return truncateHint(String(value), MAX_VALUE_HINT_LEN);
}

type StructuredEntry = readonly [string | number, StructuredValue];

export type HintValueKind = ValueKind | "null" | "nested";

export type FirstEntryHint = {
  key?: string;
  valuePreview: string;
  valueKind: HintValueKind;
};

function hintValueKind(value: StructuredValue): HintValueKind {
  if (isEmptyValue(value)) return "null";
  if (typeof value === "boolean") return "bool";
  if (typeof value === "number") return "number";
  if (typeof value === "string") return "string";
  return "nested";
}

/** Compact hint for collapsed nodes: first key/value or first array item preview. */
export function firstEntryHint(
  entries: readonly StructuredEntry[],
  isArray: boolean,
): FirstEntryHint | null {
  if (entries.length === 0) return null;
  const [key, value] = entries[0];
  return {
    key: isArray ? undefined : truncateHint(String(key), MAX_KEY_HINT_LEN),
    valuePreview: formatCompactPreview(value),
    valueKind: hintValueKind(value),
  };
}
