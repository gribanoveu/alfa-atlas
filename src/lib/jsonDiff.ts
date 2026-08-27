import { diffLines } from "diff";

export type JsonParseResult =
  | { ok: true; value: unknown }
  | { ok: false; reason: string };

export type JsonDiffKind = "add" | "remove" | "change";

export type JsonDiffChange =
  | { kind: "add"; path: string; value: unknown }
  | { kind: "remove"; path: string; value: unknown }
  | { kind: "change"; path: string; from: unknown; to: unknown };

export type JsonDiffSummary = {
  added: number;
  removed: number;
  changed: number;
  total: number;
};

export type JsonLineDiffRow = {
  kind: "context" | "add" | "remove";
  text: string;
};

export function parseJsonInput(text: string): JsonParseResult {
  const trimmed = text.trim();
  if (!trimmed) {
    return { ok: false, reason: "Введите JSON" };
  }

  try {
    return { ok: true, value: JSON.parse(trimmed) as unknown };
  } catch (error) {
    const message = error instanceof Error ? error.message : "Некорректный JSON";
    return { ok: false, reason: message };
  }
}

export function formatJson(value: unknown): string {
  return `${JSON.stringify(value, null, 2)}\n`;
}

export function formatJsonValue(value: unknown, maxLength = 120): string {
  const text = JSON.stringify(value);
  if (text.length <= maxLength) {
    return text;
  }
  return `${text.slice(0, maxLength - 1)}…`;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function jsonValuesEqual(left: unknown, right: unknown): boolean {
  if (left === right) {
    return true;
  }
  if (typeof left !== typeof right) {
    return false;
  }
  if (left === null || right === null) {
    return left === right;
  }
  if (Array.isArray(left) && Array.isArray(right)) {
    if (left.length !== right.length) {
      return false;
    }
    return left.every((item, index) => jsonValuesEqual(item, right[index]));
  }
  if (isPlainObject(left) && isPlainObject(right)) {
    const leftKeys = Object.keys(left);
    const rightKeys = Object.keys(right);
    if (leftKeys.length !== rightKeys.length) {
      return false;
    }
    return leftKeys.every((key) => key in right && jsonValuesEqual(left[key], right[key]));
  }
  return false;
}

function joinPath(base: string, segment: string): string {
  if (segment.startsWith("[")) {
    return `${base}${segment}`;
  }
  return base === "$" ? `$.${segment}` : `${base}.${segment}`;
}

export function diffJson(left: unknown, right: unknown, path = "$"): JsonDiffChange[] {
  if (jsonValuesEqual(left, right)) {
    return [];
  }

  if (Array.isArray(left) && Array.isArray(right)) {
    const changes: JsonDiffChange[] = [];
    const maxLength = Math.max(left.length, right.length);
    for (let index = 0; index < maxLength; index += 1) {
      const childPath = joinPath(path, `[${index}]`);
      if (index >= left.length) {
        changes.push({ kind: "add", path: childPath, value: right[index] });
      } else if (index >= right.length) {
        changes.push({ kind: "remove", path: childPath, value: left[index] });
      } else {
        changes.push(...diffJson(left[index], right[index], childPath));
      }
    }
    return changes;
  }

  if (isPlainObject(left) && isPlainObject(right)) {
    const changes: JsonDiffChange[] = [];
    const keys = [...new Set([...Object.keys(left), ...Object.keys(right)])].sort();
    for (const key of keys) {
      const childPath = joinPath(path, key);
      if (!(key in left)) {
        changes.push({ kind: "add", path: childPath, value: right[key] });
      } else if (!(key in right)) {
        changes.push({ kind: "remove", path: childPath, value: left[key] });
      } else {
        changes.push(...diffJson(left[key], right[key], childPath));
      }
    }
    return changes;
  }

  return [{ kind: "change", path, from: left, to: right }];
}

export function summarizeJsonDiff(changes: JsonDiffChange[]): JsonDiffSummary {
  let added = 0;
  let removed = 0;
  let changed = 0;

  for (const change of changes) {
    if (change.kind === "add") {
      added += 1;
    } else if (change.kind === "remove") {
      removed += 1;
    } else {
      changed += 1;
    }
  }

  return {
    added,
    removed,
    changed,
    total: changes.length,
  };
}

export function buildJsonLineDiff(left: unknown, right: unknown): JsonLineDiffRow[] {
  const parts = diffLines(formatJson(left), formatJson(right));
  const rows: JsonLineDiffRow[] = [];

  for (const part of parts) {
    const lines = part.value.split("\n");
    if (lines.length > 1 && lines[lines.length - 1] === "") {
      lines.pop();
    }

    const kind: JsonLineDiffRow["kind"] = part.added
      ? "add"
      : part.removed
        ? "remove"
        : "context";

    for (const text of lines) {
      rows.push({ kind, text });
    }
  }

  return rows;
}

export function formatUnifiedDiff(rows: JsonLineDiffRow[]): string {
  return rows
    .map((row) => {
      const prefix = row.kind === "add" ? "+" : row.kind === "remove" ? "-" : " ";
      return `${prefix}${row.text}`;
    })
    .join("\n");
}
