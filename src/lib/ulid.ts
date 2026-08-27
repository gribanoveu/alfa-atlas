/**
 * Генерация и разбор ULID — только чистые функции, без React и IPC.
 */

const ENCODING = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const ULID_RE = /^[0-9A-HJKMNP-TV-Z]{26}$/i;
const MAX_TIME_MS = 2 ** 48 - 1;

function randomBytes(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  globalThis.crypto.getRandomValues(bytes);
  return bytes;
}

function encodeTime(ms: number): string {
  let time = ms;
  let result = "";
  for (let index = 0; index < 10; index += 1) {
    result = ENCODING[time & 31]! + result;
    time = Math.floor(time / 32);
  }
  return result;
}

function encodeRandom(): string {
  const rand = randomBytes(10);
  return (
    ENCODING[rand[0]! >> 3]! +
    ENCODING[((rand[0]! & 0x07) << 2) | (rand[1]! >> 6)]! +
    ENCODING[(rand[1]! & 0x3e) >> 1]! +
    ENCODING[((rand[1]! & 0x01) << 4) | (rand[2]! >> 4)]! +
    ENCODING[((rand[2]! & 0x0f) << 1) | (rand[3]! >> 7)]! +
    ENCODING[(rand[3]! & 0x7c) >> 2]! +
    ENCODING[((rand[3]! & 0x03) << 3) | (rand[4]! >> 5)]! +
    ENCODING[rand[4]! & 0x1f]! +
    ENCODING[rand[5]! >> 3]! +
    ENCODING[((rand[5]! & 0x07) << 2) | (rand[6]! >> 6)]! +
    ENCODING[(rand[6]! & 0x3e) >> 1]! +
    ENCODING[((rand[6]! & 0x01) << 4) | (rand[7]! >> 4)]! +
    ENCODING[((rand[7]! & 0x0f) << 1) | (rand[8]! >> 7)]! +
    ENCODING[(rand[8]! & 0x7c) >> 2]! +
    ENCODING[((rand[8]! & 0x03) << 3) | (rand[9]! >> 5)]! +
    ENCODING[rand[9]! & 0x1f]!
  );
}

/** ULID в верхнем регистре (каноничный вид Crockford Base32). */
export function generateUlid(nowMs: number = Date.now()): string {
  const ms = Math.trunc(nowMs);
  if (ms < 0 || ms > MAX_TIME_MS) {
    throw new RangeError("ULID timestamp out of range");
  }
  return encodeTime(ms) + encodeRandom();
}

export function generateUlidBatch(count: number, nowMs: number = Date.now()): string[] {
  const safeCount = Math.max(0, Math.min(Math.trunc(count), 100));
  return Array.from({ length: safeCount }, () => generateUlid(nowMs));
}

/** Миллисекунды Unix из первых 10 символов ULID, или null при неверном формате. */
export function decodeUlidTimestamp(ulid: string): number | null {
  const normalized = ulid.trim().toUpperCase();
  if (!ULID_RE.test(normalized)) return null;

  let time = 0;
  for (let index = 0; index < 10; index += 1) {
    const char = normalized[index]!;
    const value = ENCODING.indexOf(char);
    if (value === -1) return null;
    time = time * 32 + value;
  }
  return time;
}

export function isUlid(value: string): boolean {
  return ULID_RE.test(value.trim());
}
