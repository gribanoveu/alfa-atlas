import { describe, expect, test } from "bun:test";
import {
  decodeUlidTimestamp,
  generateUlid,
  generateUlidBatch,
  isUlid,
} from "../lib/ulid";

describe("ulid", () => {
  test("generateUlid возвращает 26-символьный идентификатор", () => {
    const value = generateUlid(1_700_000_000_000);
    expect(value).toHaveLength(26);
    expect(isUlid(value)).toBe(true);
    expect(value).toBe(value.toUpperCase());
  });

  test("decodeUlidTimestamp читает ту же метку времени", () => {
    const ms = 1_700_000_000_000;
    const value = generateUlid(ms);
    expect(decodeUlidTimestamp(value)).toBe(ms);
  });

  test("batch генерирует нужное количество ULID", () => {
    expect(generateUlidBatch(3, 1_700_000_000_000)).toHaveLength(3);
    expect(generateUlidBatch(1000)).toHaveLength(100);
  });
});
