import { describe, expect, test } from "bun:test";
import { generateUuidV4, generateUuidV4Batch, isUuidV4 } from "../lib/uuid";

describe("uuid", () => {
  test("generateUuidV4 возвращает валидный UUID v4", () => {
    const value = generateUuidV4();
    expect(isUuidV4(value)).toBe(true);
    expect(value).toBe(value.toLowerCase());
  });

  test("batch ограничивается сотней и не содержит дубликатов в маленькой выборке", () => {
    const values = generateUuidV4Batch(5);
    expect(values).toHaveLength(5);
    expect(new Set(values).size).toBe(5);
    expect(generateUuidV4Batch(1000)).toHaveLength(100);
  });
});
