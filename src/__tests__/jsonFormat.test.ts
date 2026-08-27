import { describe, expect, test } from "bun:test";
import { formatJsonInput, formatJsonValue, sortJsonValue } from "../lib/jsonFormat";

describe("jsonFormat", () => {
  test("formatJsonValue prettify с отступом 2", () => {
    const output = formatJsonValue({ a: 1, b: [2] }, { mode: "prettify", indent: 2, sortKeys: false });
    expect(output).toBe('{\n  "a": 1,\n  "b": [\n    2\n  ]\n}\n');
  });

  test("formatJsonValue minify в одну строку", () => {
    const output = formatJsonValue({ a: 1, b: 2 }, { mode: "minify", indent: 2, sortKeys: false });
    expect(output).toBe('{"a":1,"b":2}');
  });

  test("sortJsonValue сортирует ключи рекурсивно", () => {
    const sorted = sortJsonValue({ z: 1, a: { y: 2, b: 3 } });
    expect(Object.keys(sorted as Record<string, unknown>)).toEqual(["a", "z"]);
    expect(Object.keys((sorted as { a: Record<string, unknown> }).a)).toEqual(["b", "y"]);
  });

  test("formatJsonInput возвращает ошибку для некорректного JSON", () => {
    const result = formatJsonInput("{bad", { mode: "prettify", indent: 2, sortKeys: false });
    expect(result.ok).toBe(false);
  });

  test("formatJsonInput считает размер входа и выхода", () => {
    const result = formatJsonInput('{"b":2,"a":1}', {
      mode: "prettify",
      indent: 2,
      sortKeys: true,
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.output).toContain('"a": 1');
      expect(result.bytesIn).toBeGreaterThan(0);
      expect(result.bytesOut).toBeGreaterThan(result.bytesIn);
    }
  });
});
