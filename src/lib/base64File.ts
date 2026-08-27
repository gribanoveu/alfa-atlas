import type { Base64Alphabet } from "./base64Codec";

export const MAX_BASE64_FILE_BYTES = 10 * 1024 * 1024;

export type BinaryContentKind = "pdf" | "image" | "unknown";

export type BinaryContentInfo = {
  kind: BinaryContentKind;
  mime: string;
  extension: string;
  label: string;
};

export type ParsedDataUri = {
  mime: string;
  base64: string;
};

export type Base64FileDecodeOptions = {
  alphabet: Base64Alphabet;
};

export type Base64FileDecodeResult =
  | {
      ok: true;
      bytes: Uint8Array;
      bytesIn: number;
      content: BinaryContentInfo;
    }
  | { ok: false; reason: string };

export type Base64FileEncodeResult =
  | {
      ok: true;
      base64: string;
      bytesIn: number;
      bytesOut: number;
      fileName: string;
      content: BinaryContentInfo;
    }
  | { ok: false; reason: string };

function readAscii(bytes: Uint8Array, length: number): string {
  let text = "";
  for (let index = 0; index < length; index += 1) {
    text += String.fromCharCode(bytes[index]!);
  }
  return text;
}

function startsWith(bytes: Uint8Array, signature: number[]): boolean {
  if (bytes.length < signature.length) {
    return false;
  }
  return signature.every((value, index) => bytes[index] === value);
}

export function detectBinaryContent(bytes: Uint8Array): BinaryContentInfo {
  if (startsWith(bytes, [0x25, 0x50, 0x44, 0x46])) {
    return {
      kind: "pdf",
      mime: "application/pdf",
      extension: "pdf",
      label: "PDF",
    };
  }

  if (startsWith(bytes, [0x89, 0x50, 0x4e, 0x47])) {
    return {
      kind: "image",
      mime: "image/png",
      extension: "png",
      label: "PNG",
    };
  }

  if (startsWith(bytes, [0xff, 0xd8, 0xff])) {
    return {
      kind: "image",
      mime: "image/jpeg",
      extension: "jpg",
      label: "JPEG",
    };
  }

  if (readAscii(bytes, 6) === "GIF87a" || readAscii(bytes, 6) === "GIF89a") {
    return {
      kind: "image",
      mime: "image/gif",
      extension: "gif",
      label: "GIF",
    };
  }

  if (
    startsWith(bytes, [0x52, 0x49, 0x46, 0x46]) &&
    bytes.length >= 12 &&
    readAscii(bytes.subarray(8, 12), 4) === "WEBP"
  ) {
    return {
      kind: "image",
      mime: "image/webp",
      extension: "webp",
      label: "WebP",
    };
  }

  return {
    kind: "unknown",
    mime: "application/octet-stream",
    extension: "bin",
    label: "Бинарный файл",
  };
}

export function parseDataUri(input: string): ParsedDataUri | null {
  const match = input.trim().match(/^data:([^;,]+)?(?:;[^,]*)?;base64,(.+)$/i);
  if (!match?.[2]) {
    return null;
  }

  return {
    mime: match[1]?.trim() || "application/octet-stream",
    base64: match[2].replace(/\s+/g, ""),
  };
}

export function contentFromMime(mime: string): BinaryContentInfo {
  const normalized = mime.trim().toLowerCase();
  if (normalized === "application/pdf") {
    return {
      kind: "pdf",
      mime: "application/pdf",
      extension: "pdf",
      label: "PDF",
    };
  }
  if (normalized.startsWith("image/")) {
    const extension = normalized.slice("image/".length) || "img";
    return {
      kind: "image",
      mime: normalized,
      extension: extension === "jpeg" ? "jpg" : extension,
      label: extension.toUpperCase(),
    };
  }

  return {
    kind: "unknown",
    mime: normalized,
    extension: "bin",
    label: normalized,
  };
}

export function decodeBase64FileInput(
  input: string,
  options: Base64FileDecodeOptions,
  decodeBase64: (value: string, alphabet: Base64Alphabet) => Uint8Array,
): Base64FileDecodeResult {
  const trimmed = input.trim();
  if (!trimmed) {
    return { ok: false, reason: "Введите Base64" };
  }

  const dataUri = parseDataUri(trimmed);
  const base64 = dataUri?.base64 ?? trimmed;

  try {
    const bytes = decodeBase64(base64, options.alphabet);
    if (bytes.length > MAX_BASE64_FILE_BYTES) {
      return {
        ok: false,
        reason: `Файл больше ${formatMegabytes(MAX_BASE64_FILE_BYTES)} — уменьшите размер или сохраните без предпросмотра`,
      };
    }

    const content = dataUri ? contentFromMime(dataUri.mime) : detectBinaryContent(bytes);

    return {
      ok: true,
      bytes,
      bytesIn: base64.replace(/\s+/g, "").length,
      content,
    };
  } catch {
    return { ok: false, reason: "Некорректная Base64-строка" };
  }
}

export function encodeFileBytesToBase64(
  bytes: Uint8Array,
  fileName: string,
  alphabet: Base64Alphabet,
  padding: boolean,
  encodeBase64: (value: Uint8Array, alphabet: Base64Alphabet, padding: boolean) => string,
): Base64FileEncodeResult {
  if (bytes.length === 0) {
    return { ok: false, reason: "Файл пустой" };
  }

  if (bytes.length > MAX_BASE64_FILE_BYTES) {
    return {
      ok: false,
      reason: `Файл больше ${formatMegabytes(MAX_BASE64_FILE_BYTES)}`,
    };
  }

  const base64 = encodeBase64(bytes, alphabet, padding);
  return {
    ok: true,
    base64,
    bytesIn: bytes.length,
    bytesOut: new TextEncoder().encode(base64).length,
    fileName,
    content: detectBinaryContent(bytes),
  };
}

export function formatMegabytes(bytes: number): string {
  return `${Math.round(bytes / (1024 * 1024))} MB`;
}

export function suggestedFileName(content: BinaryContentInfo, fallback = "decoded"): string {
  return `${fallback}.${content.extension}`;
}

export function createObjectUrl(bytes: Uint8Array, mime: string): string {
  const blob = new Blob([bytes], { type: mime });
  return URL.createObjectURL(blob);
}

export async function readFileAsBytes(file: File): Promise<{ name: string; bytes: Uint8Array }> {
  const buffer = await file.arrayBuffer();
  return {
    name: file.name,
    bytes: new Uint8Array(buffer),
  };
}
