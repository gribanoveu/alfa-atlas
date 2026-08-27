import { describe, expect, test } from "bun:test";
import { formatXmlInput, formatXmlValue } from "../lib/xmlFormat";

describe("xmlFormat", () => {
  const sample = "<root><item><name>Alpha</name></item></root>";

  test("formatXmlValue prettify добавляет отступы", () => {
    const output = formatXmlValue(sample, { mode: "prettify", indent: 2 });
    expect(output).toContain("<root>\n");
    expect(output).toContain('  <item>');
  });

  test("formatXmlValue minify сжимает XML", () => {
    const pretty = formatXmlValue(sample, { mode: "prettify", indent: 2 });
    const minified = formatXmlValue(pretty, { mode: "minify", indent: 2 });
    expect(minified).not.toContain("\n");
    expect(minified).toContain("<root><item><name>Alpha</name></item></root>");
  });

  test("formatXmlInput возвращает ошибку для пустого ввода", () => {
    const result = formatXmlInput("   ", { mode: "prettify", indent: 2 });
    expect(result.ok).toBe(false);
  });

  test("formatXmlInput возвращает ошибку для некорректного XML", () => {
    const result = formatXmlInput("<root><item>", { mode: "prettify", indent: 2 });
    expect(result.ok).toBe(false);
  });

  test("formatXmlInput считает размер входа и выхода", () => {
    const result = formatXmlInput(sample, { mode: "prettify", indent: 2 });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.bytesIn).toBeGreaterThan(0);
      expect(result.bytesOut).toBeGreaterThan(result.bytesIn);
    }
  });
});
