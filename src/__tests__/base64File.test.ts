import { describe, expect, test } from "bun:test";
import {
  decodeBase64FileInput,
  detectBinaryContent,
  encodeFileBytesToBase64,
  parseDataUri,
} from "../lib/base64File";
import { decodeBase64ToBytes, encodeBytesToBase64 } from "../lib/base64Codec";

const TINY_PNG_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAD0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

describe("base64File", () => {
  test("detectBinaryContent определяет PNG и PDF", () => {
    const png = decodeBase64ToBytes(TINY_PNG_BASE64, "standard");
    expect(detectBinaryContent(png).kind).toBe("image");
    expect(detectBinaryContent(png).extension).toBe("png");

    const pdf = new TextEncoder().encode("%PDF-1.4");
    expect(detectBinaryContent(pdf).kind).toBe("pdf");
  });

  test("parseDataUri извлекает mime и base64", () => {
    const parsed = parseDataUri("data:image/png;base64,QUJD");
    expect(parsed).toEqual({ mime: "image/png", base64: "QUJD" });
  });

  test("decodeBase64FileInput декодирует PNG", () => {
    const result = decodeBase64FileInput(TINY_PNG_BASE64, { alphabet: "standard" }, decodeBase64ToBytes);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.content.kind).toBe("image");
      expect(result.bytes.length).toBeGreaterThan(0);
    }
  });

  test("decodeBase64FileInput понимает data URI", () => {
    const result = decodeBase64FileInput(
      `data:image/png;base64,${TINY_PNG_BASE64}`,
      { alphabet: "standard" },
      decodeBase64ToBytes,
    );
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.content.mime).toBe("image/png");
    }
  });

  test("encodeFileBytesToBase64 кодирует файл", () => {
    const bytes = decodeBase64ToBytes(TINY_PNG_BASE64, "standard");
    const result = encodeFileBytesToBase64(
      bytes,
      "pixel.png",
      "standard",
      true,
      encodeBytesToBase64,
    );
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.base64).toBe(TINY_PNG_BASE64);
      expect(result.fileName).toBe("pixel.png");
    }
  });
});
