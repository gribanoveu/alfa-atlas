import { parseJsonInput } from "./jsonDiff";

export type JsonIndent = 2 | 4;

export type JsonFormatMode = "prettify" | "minify";

export type JsonFormatOptions = {
  mode: JsonFormatMode;
  indent: JsonIndent;
  sortKeys: boolean;
};

export type JsonFormatResult =
  | {
      ok: true;
      output: string;
      bytesIn: number;
      bytesOut: number;
    }
  | { ok: false; reason: string };

export function sortJsonValue(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(sortJsonValue);
  }

  if (typeof value === "object" && value !== null) {
    const object = value as Record<string, unknown>;
    const sorted: Record<string, unknown> = {};
    for (const key of Object.keys(object).sort()) {
      sorted[key] = sortJsonValue(object[key]);
    }
    return sorted;
  }

  return value;
}

export function formatJsonValue(value: unknown, options: JsonFormatOptions): string {
  const prepared = options.sortKeys ? sortJsonValue(value) : value;

  if (options.mode === "minify") {
    return JSON.stringify(prepared);
  }

  return `${JSON.stringify(prepared, null, options.indent)}\n`;
}

export function formatJsonInput(text: string, options: JsonFormatOptions): JsonFormatResult {
  const parsed = parseJsonInput(text);
  if (!parsed.ok) {
    return parsed;
  }

  const output = formatJsonValue(parsed.value, options);
  const bytesIn = new TextEncoder().encode(text.trim()).length;
  const bytesOut = new TextEncoder().encode(output).length;

  return {
    ok: true,
    output,
    bytesIn,
    bytesOut,
  };
}
