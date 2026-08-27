import { describe, expect, test } from "bun:test";
import { decodeBase64String, encodeBase64String } from "../lib/base64Codec";
import {
  decodeBytesAsText,
  detectTextEncoding,
  encodeWindows1251ForTest,
  normalizeEncodingName,
} from "../lib/textEncoding";

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}

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
    const result = decodeBase64String("0J/RgNC40LLQtdGCLCBkb2NmbG93IQ==", {
      alphabet: "standard",
      encoding: "auto",
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.output).toBe("Привет, docflow!");
    }
  });

  test("decodeBase64String читает XML ФНС с windows-1251 из декларации", () => {
    const xml =
      '<?xml version="1.0" encoding="windows-1251"?><Файл><Сведения>Тест</Сведения></Файл>';
    const base64 = bytesToBase64(encodeWindows1251ForTest(xml));

    const result = decodeBase64String(base64, { alphabet: "standard", encoding: "auto" });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.output).toContain("<Файл>");
      expect(result.output).toContain("Тест");
      expect(result.encodingLabel).toContain("Windows-1251");
      expect(result.encodingLabel).toContain("XML");
    }
  });

  test("decodeBase64String декодирует cp1251 по эвристике", () => {
    const base64 = bytesToBase64(encodeWindows1251ForTest("Тест"));

    const result = decodeBase64String(base64, {
      alphabet: "standard",
      encoding: "auto",
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.output).toBe("Тест");
      expect(result.encodingLabel).toContain("Windows-1251");
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

    const decoded = decodeBase64String(encoded.output, {
      alphabet: "url",
      encoding: "utf-8",
    });
    expect(decoded.ok).toBe(true);
    if (decoded.ok) {
      expect(decoded.output).toBe("hello/world?");
    }
  });

  test("decodeBase64String отклоняет некорректную строку", () => {
    const result = decodeBase64String("***", { alphabet: "standard", encoding: "auto" });
    expect(result.ok).toBe(false);
  });

  test("encodeBase64String требует непустой ввод", () => {
    const result = encodeBase64String("", { alphabet: "standard", padding: true });
    expect(result.ok).toBe(false);
  });
});

describe("textEncoding", () => {
  test("normalizeEncodingName понимает cp1251", () => {
    expect(normalizeEncodingName("CP1251")).toBe("windows-1251");
  });

  test("detectTextEncoding читает encoding из XML", () => {
    const bytes = new TextEncoder().encode('<?xml version="1.0" encoding="windows-1251"?><x/>');
    const detected = detectTextEncoding(bytes);
    expect(detected.encoding).toBe("windows-1251");
    expect(detected.source).toBe("xml-declaration");
  });

  test("decodeBytesAsText декодирует windows-1251 явно", () => {
    const bytes = encodeWindows1251ForTest("Тест");
    const decoded = decodeBytesAsText(bytes, "windows-1251");
    expect(decoded.text).toBe("Тест");
  });
});
