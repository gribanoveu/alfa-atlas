export type TextEncodingId = "auto" | "utf-8" | "windows-1251" | "iso-8859-5" | "koi8-r";

export type TextEncodingOption = {
  id: TextEncodingId;
  label: string;
};

export const TEXT_ENCODING_OPTIONS: TextEncodingOption[] = [
  { id: "auto", label: "Авто" },
  { id: "utf-8", label: "UTF-8" },
  { id: "windows-1251", label: "Windows-1251" },
  { id: "iso-8859-5", label: "ISO-8859-5" },
  { id: "koi8-r", label: "KOI8-R" },
];

const ENCODING_LABELS: Record<Exclude<TextEncodingId, "auto">, string> = {
  "utf-8": "UTF-8",
  "windows-1251": "Windows-1251",
  "iso-8859-5": "ISO-8859-5",
  "koi8-r": "KOI8-R",
};

const TEXT_DECODER_LABELS: Record<Exclude<TextEncodingId, "auto">, string> = {
  "utf-8": "utf-8",
  "windows-1251": "windows-1251",
  "iso-8859-5": "iso-8859-5",
  "koi8-r": "koi8-r",
};

/** Таблица символов для байтов 128–255 (как в iconv-lite/windows-1251). */
const WINDOWS1251_HIGH =
  "ЂЃ‚ѓ„…†‡€‰Љ‹ЊЌЋЏђ‘’“”•–—�™љ›њќћџ Ў¢Ј¤Ґ¦§Ё©Є«¬­®Ї°±Ііґµ¶·ё№є»јЅѕїАБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдежзийклмнопрстуфхцчшщъыьэюя";

const ENCODING_ALIASES: Record<string, Exclude<TextEncodingId, "auto">> = {
  "utf-8": "utf-8",
  utf8: "utf-8",
  "windows-1251": "windows-1251",
  cp1251: "windows-1251",
  "win-1251": "windows-1251",
  "iso-8859-5": "iso-8859-5",
  iso88595: "iso-8859-5",
  cyrillic: "iso-8859-5",
  "koi8-r": "koi8-r",
  koi8r: "koi8-r",
};

export type DetectedTextEncoding = {
  encoding: Exclude<TextEncodingId, "auto">;
  label: string;
  source: "bom" | "xml-declaration" | "utf-8-valid" | "heuristic";
};

function readLatin1(bytes: Uint8Array, maxLength = 512): string {
  const length = Math.min(bytes.length, maxLength);
  let text = "";
  for (let index = 0; index < length; index += 1) {
    text += String.fromCharCode(bytes[index]!);
  }
  return text;
}

function decodeSbcs(bytes: Uint8Array, highChars: string): string {
  let text = "";
  for (const byte of bytes) {
    if (byte < 128) {
      text += String.fromCharCode(byte);
    } else {
      text += highChars.charAt(byte - 128);
    }
  }
  return text;
}

function decodeWithTextDecoder(bytes: Uint8Array, encoding: Exclude<TextEncodingId, "auto">): string {
  return new TextDecoder(TEXT_DECODER_LABELS[encoding], { fatal: true }).decode(bytes);
}

function decodeWithEncoding(
  bytes: Uint8Array,
  encoding: Exclude<TextEncodingId, "auto">,
): string {
  if (encoding === "windows-1251") {
    return decodeSbcs(bytes, WINDOWS1251_HIGH);
  }

  if (encoding === "utf-8") {
    return decodeWithTextDecoder(bytes, encoding);
  }

  try {
    return decodeWithTextDecoder(bytes, encoding);
  } catch {
    throw new Error(`Кодировка ${ENCODING_LABELS[encoding]} не поддерживается`);
  }
}

export function normalizeEncodingName(name: string): Exclude<TextEncodingId, "auto"> | null {
  const normalized = name.trim().toLowerCase().replace(/[_\s]/g, "-");
  return ENCODING_ALIASES[normalized] ?? null;
}

function detectBom(bytes: Uint8Array): Exclude<TextEncodingId, "auto"> | null {
  if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
    return "utf-8";
  }
  return null;
}

function parseXmlEncoding(bytes: Uint8Array): Exclude<TextEncodingId, "auto"> | null {
  const prolog = readLatin1(bytes);
  const match = prolog.match(/<\?xml\b[^?]*\bencoding=["']([^"']+)["']/i);
  if (!match?.[1]) {
    return null;
  }
  return normalizeEncodingName(match[1]);
}

function isValidUtf8(bytes: Uint8Array): boolean {
  try {
    new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return true;
  } catch {
    return false;
  }
}

export function detectTextEncoding(bytes: Uint8Array): DetectedTextEncoding {
  const bom = detectBom(bytes);
  if (bom) {
    return {
      encoding: bom,
      label: ENCODING_LABELS[bom],
      source: "bom",
    };
  }

  const xmlEncoding = parseXmlEncoding(bytes);
  if (xmlEncoding) {
    return {
      encoding: xmlEncoding,
      label: ENCODING_LABELS[xmlEncoding],
      source: "xml-declaration",
    };
  }

  if (isValidUtf8(bytes)) {
    return {
      encoding: "utf-8",
      label: ENCODING_LABELS["utf-8"],
      source: "utf-8-valid",
    };
  }

  return {
    encoding: "windows-1251",
    label: `${ENCODING_LABELS["windows-1251"]} (эвристика)`,
    source: "heuristic",
  };
}

export function decodeBytesAsText(
  bytes: Uint8Array,
  encoding: TextEncodingId,
): { text: string; detected: DetectedTextEncoding } {
  const detected =
    encoding === "auto"
      ? detectTextEncoding(bytes)
      : {
          encoding,
          label: ENCODING_LABELS[encoding],
          source: "utf-8-valid" as const,
        };

  const text = decodeWithEncoding(bytes, detected.encoding);
  return { text, detected };
}

export function formatDetectedEncoding(detected: DetectedTextEncoding): string {
  if (detected.source === "xml-declaration") {
    return `${detected.label} (из XML)`;
  }
  if (detected.source === "bom") {
    return `${detected.label} (BOM)`;
  }
  return detected.label;
}

/** Только для тестов: кодирует текст в Windows-1251. */
export function encodeWindows1251ForTest(text: string): Uint8Array {
  const bytes: number[] = [];
  for (const char of text) {
    const code = char.charCodeAt(0);
    if (code < 128) {
      bytes.push(code);
      continue;
    }

    const index = WINDOWS1251_HIGH.indexOf(char);
    if (index === -1) {
      throw new Error(`Символ не представим в Windows-1251: ${char}`);
    }
    bytes.push(index + 128);
  }
  return Uint8Array.from(bytes);
}
