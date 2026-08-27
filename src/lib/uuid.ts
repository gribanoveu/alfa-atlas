/**
 * Генерация UUID v4 — только чистые функции, без React и IPC.
 */

const UUID_V4_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

function randomBytes(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  globalThis.crypto.getRandomValues(bytes);
  return bytes;
}

function formatUuidV4(bytes: Uint8Array): string {
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return (
    `${hex.slice(0, 8)}-${hex.slice(8, 12)}-` +
    `${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
  );
}

/** Случайный UUID версии 4 в каноническом нижнем регистре. */
export function generateUuidV4(): string {
  if (typeof globalThis.crypto.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }

  const bytes = randomBytes(16);
  bytes[6] = (bytes[6]! & 0x0f) | 0x40;
  bytes[8] = (bytes[8]! & 0x3f) | 0x80;
  return formatUuidV4(bytes);
}

export function generateUuidV4Batch(count: number): string[] {
  const safeCount = Math.max(0, Math.min(Math.trunc(count), 100));
  return Array.from({ length: safeCount }, () => generateUuidV4());
}

export function isUuidV4(value: string): boolean {
  return UUID_V4_RE.test(value.trim());
}
