import { describe, expect, test } from "bun:test";
import {
  buildJsonLineDiff,
  diffJson,
  formatUnifiedDiff,
  parseJsonInput,
  summarizeJsonDiff,
} from "../lib/jsonDiff";

describe("jsonDiff", () => {
  test("parseJsonInput принимает валидный JSON", () => {
    const result = parseJsonInput('{"a":1}');
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value).toEqual({ a: 1 });
    }
  });

  test("parseJsonInput отклоняет некорректный JSON", () => {
    const result = parseJsonInput("{bad");
    expect(result.ok).toBe(false);
  });

  test("diffJson находит добавление, удаление и изменение", () => {
    const left = { keep: 1, old: true, nested: { x: 1 } };
    const right = { keep: 1, fresh: "new", nested: { x: 2 } };

    const changes = diffJson(left, right);
    expect(changes.some((change) => change.kind === "remove" && change.path === "$.old")).toBe(
      true,
    );
    expect(changes.some((change) => change.kind === "add" && change.path === "$.fresh")).toBe(
      true,
    );
    expect(
      changes.some((change) => change.kind === "change" && change.path === "$.nested.x"),
    ).toBe(true);
  });

  test("diffJson сравнивает элементы массива по индексу", () => {
    const changes = diffJson([1, 2], [1, 3, 4]);
    expect(changes).toEqual([
      { kind: "change", path: "$[1]", from: 2, to: 3 },
      { kind: "add", path: "$[2]", value: 4 },
    ]);
  });

  test("summarizeJsonDiff считает типы изменений", () => {
    const summary = summarizeJsonDiff([
      { kind: "add", path: "$.a", value: 1 },
      { kind: "remove", path: "$.b", value: 2 },
      { kind: "change", path: "$.c", from: 1, to: 2 },
    ]);
    expect(summary).toEqual({ added: 1, removed: 1, changed: 1, total: 3 });
  });

  test("buildJsonLineDiff и formatUnifiedDiff формируют построчный diff", () => {
    const rows = buildJsonLineDiff({ a: 1 }, { a: 2 });
    const text = formatUnifiedDiff(rows);
    expect(text).toContain("-  \"a\": 1");
    expect(text).toContain("+  \"a\": 2");
  });
});
