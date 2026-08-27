import { describe, expect, test } from "bun:test";
import { decodeBase64String, encodeBase64String } from "../lib/base64Codec";

describe("base64Codec", () => {
  test("encodeBase64String кодирует UTF-8 текст", () => {
    const result = encodeBase64String("docflow", {
      alphabet: "standard",
      padding: true,
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.output).toBe("ZG9jZmxvdw==");
    }
  });

  test("decodeBase64String декодирует UTF-8 текст", () => {
    const result = decodeBase64String("0J/RgNC40LLQtdGCLCBkb2NmbG93IQ==", "standard");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.output).toBe("Привет, atlas!");
    }
  });

  test("encode и decode roundtrip для URL-safe без padding", () => {
    const encoded = encodeBase64String("hello/world?", {
      alphabet: "url",
      padding: false,
    });
    expect(encoded.ok).toBe(true);
    if (!encoded.ok) {
      return;
    }

    const decoded = decodeBase64String(encoded.output, "url");
    expect(decoded.ok).toBe(true);
    if (decoded.ok) {
      expect(decoded.output).toBe("hello/world?");
    }
  });

  test("decodeBase64String отклоняет некорректную строку", () => {
    const result = decodeBase64String("***", "standard");
    expect(result.ok).toBe(false);
  });

  test("encodeBase64String требует непустой ввод", () => {
    const result = encodeBase64String("", { alphabet: "standard", padding: true });
    expect(result.ok).toBe(false);
  });
});
