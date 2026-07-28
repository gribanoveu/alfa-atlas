import { describe, expect, test } from "bun:test";
import {
  extensionOf,
  isJsonPath,
  isMarkdownPath,
  isYamlPath,
} from "../lib/fileExtensions";
import {
  collectPaths,
  firstEntryHint,
  parseStructuredData,
  valueKind,
} from "../components/StructuredDataPreview/structuredDataUtils";

describe("fileExtensions structured data", () => {
  test("isJsonPath detects json files", () => {
    expect(isJsonPath("config.json")).toBe(true);
    expect(isJsonPath("nested/app.JSON")).toBe(true);
    expect(isJsonPath("readme.yaml")).toBe(false);
  });

  test("isYamlPath detects yaml files", () => {
    expect(isYamlPath("config.yaml")).toBe(true);
    expect(isYamlPath("values.YML")).toBe(true);
    expect(isYamlPath("data.json")).toBe(false);
  });

  test("extensionOf still works for mixed paths", () => {
    expect(extensionOf("docs/readme.md")).toBe(".md");
    expect(isMarkdownPath("docs/readme.md")).toBe(true);
  });
});

describe("structuredDataUtils", () => {
  test("parseStructuredData parses valid json", () => {
    const result = parseStructuredData('{"name":"alfa-atlas","count":2}', "app.json");
    expect(result.error).toBeNull();
    expect(result.data).toEqual({ name: "alfa-atlas", count: 2 });
  });

  test("parseStructuredData returns error for invalid json", () => {
    const result = parseStructuredData("{bad json", "app.json");
    expect(result.data).toBeNull();
    expect(result.error).toBeTruthy();
  });

  test("parseStructuredData parses simple yaml", () => {
    const result = parseStructuredData(
      "service: wowtax\nenabled: true\ncount: 3",
      "service.yaml",
    );
    expect(result.error).toBeNull();
    expect(result.data).toEqual({
      service: "wowtax",
      enabled: true,
      count: 3,
    });
  });

  test("parseStructuredData treats empty content as null data", () => {
    const result = parseStructuredData("   \n", "empty.json");
    expect(result.error).toBeNull();
    expect(result.data).toBeNull();
  });

  test("collectPaths gathers nested object paths", () => {
    const data = {
      kafka: {
        topics: [{ name: "events" }],
      },
    };
    const paths = collectPaths(data, "root");
    expect(paths.has("root")).toBe(true);
    expect(paths.has("root/kafka")).toBe(true);
    expect(paths.has("root/kafka/topics")).toBe(true);
    expect(paths.has("root/kafka/topics/0")).toBe(true);
  });

  test("valueKind classifies primitives", () => {
    expect(valueKind(true)).toBe("bool");
    expect(valueKind(false)).toBe("bool");
    expect(valueKind(42)).toBe("number");
    expect(valueKind("text")).toBe("string");
  });

  test("firstEntryHint returns first object key and value", () => {
    const hint = firstEntryHint(
      [
        ["service", "wowtax-notifier"],
        ["version", "1.0"],
      ],
      false,
    );
    expect(hint).toEqual({
      key: "service",
      valuePreview: '"wowtax-notifier"',
      valueKind: "string",
    });
  });

  test("firstEntryHint returns compact preview for array items", () => {
    expect(firstEntryHint([[0, "java21"], [1, "kafka"]], true)).toEqual({
      valuePreview: '"java21"',
      valueKind: "string",
    });
    expect(firstEntryHint([[0, { name: "events" }]], true)).toEqual({
      valuePreview: "name",
      valueKind: "nested",
    });
  });
});
