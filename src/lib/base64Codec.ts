import {
  decodeBytesAsText,
  formatDetectedEncoding,
  type TextEncodingId,
} from "./textEncoding";

export type Base64Alphabet = "standard" | "url";

export type Base64EncodeOptions = {
  alphabet: Base64Alphabet;
  padding: boolean;
};

export type Base64DecodeOptions = {
  alphabet: Base64Alphabet;
  encoding: TextEncodingId;
};

export type Base64CodecResult =
  | {
      ok: true;
      output: string;
      bytesIn: number;
      bytesOut: number;
      encodingLabel?: string;
    }
  | { ok: false; reason: string };

const STANDARD_BASE64_PATTERN = /^[A-Za-z0-9+/]*={0,2}$/;
const URL_BASE64_PATTERN = /^[A-Za-z0-9_-]*={0,2}$/;

function stripWhitespace(text: string): string {
  return text.replace(/\s+/g, "");
}

function normalizeBase64Input(input: string, alphabet: Base64Alphabet): string {
  let text = stripWhitespace(input);
  if (alphabet === "url") {
    text = text.replace(/-/g, "+").replace(/_/g, "/");
  }
  const padLength = (4 - (text.length % 4)) % 4;
  return `${text}${"=".repeat(padLength)}`;
}

function isValidBase64Input(input: string, alphabet: Base64Alphabet): boolean {
  const text = stripWhitespace(input);
  if (!text) {
    return false;
  }

  const pattern = alphabet === "url" ? URL_BASE64_PATTERN : STANDARD_BASE64_PATTERN;
  return pattern.test(text);
}

function bytesToBase64(bytes: Uint8Array, alphabet: Base64Alphabet, padding: boolean): string {
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }

  let encoded = btoa(binary);
  if (alphabet === "url") {
    encoded = encoded.replace(/\+/g, "-").replace(/\//g, "_");
  }
  if (!padding) {
    encoded = encoded.replace(/=+$/, "");
  }

  return encoded;
}

export function encodeBytesToBase64(
  bytes: Uint8Array,
  alphabet: Base64Alphabet,
  padding: boolean,
): string {
  return bytesToBase64(bytes, alphabet, padding);
}

export function stripBase64Whitespace(text: string): string {
  return stripWhitespace(text);
}

export function isValidBase64Text(input: string, alphabet: Base64Alphabet): boolean {
  return isValidBase64Input(input, alphabet);
}

export function decodeBase64ToBytes(input: string, alphabet: Base64Alphabet): Uint8Array {
  const normalized = normalizeBase64Input(input, alphabet);
  const binary = atob(normalized);
  return Uint8Array.from(binary, (char) => char.charCodeAt(0));
}

export function encodeBase64String(
  text: string,
  options: Base64EncodeOptions,
): Base64CodecResult {
  if (!text) {
    return { ok: false, reason: "Введите текст" };
  }

  try {
    const bytes = new TextEncoder().encode(text);
    const output = bytesToBase64(bytes, options.alphabet, options.padding);

    return {
      ok: true,
      output,
      bytesIn: bytes.length,
      bytesOut: new TextEncoder().encode(output).length,
    };
  } catch {
    return { ok: false, reason: "Не удалось закодировать текст" };
  }
}

export function decodeBase64String(
  input: string,
  options: Base64DecodeOptions,
): Base64CodecResult {
  const trimmed = input.trim();
  if (!trimmed) {
    return { ok: false, reason: "Введите Base64" };
  }

  if (!isValidBase64Input(trimmed, options.alphabet)) {
    return { ok: false, reason: "Некорректная Base64-строка" };
  }

  try {
    const bytes = decodeBase64ToBytes(trimmed, options.alphabet);
    const decoded = decodeBytesAsText(bytes, options.encoding);
    const bytesIn = stripWhitespace(trimmed).length;

    return {
      ok: true,
      output: decoded.text,
      bytesIn,
      bytesOut: bytes.length,
      encodingLabel: formatDetectedEncoding(decoded.detected),
    };
  } catch (error) {
    const message = error instanceof Error ? error.message : "Не удалось декодировать текст";
    return { ok: false, reason: message };
  }
}
