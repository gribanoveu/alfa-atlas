import { describe, expect, test } from "bun:test";
import {
  decodeUnix,
  detectUnixUnit,
  formatIsoLocal,
  formatIsoUtc,
  formatSeconds,
  partsFromDate,
  partsFromStrings,
  partsToUnix,
} from "../lib/unixtime";

/** Ошибку удобнее сверять по тексту, а тип-гард сузит union. */
function reasonOf(result: { ok: boolean; reason?: string }): string {
  return result.ok ? "" : (result.reason ?? "");
}

describe("detectUnixUnit", () => {
  test("десятизначные метки — это секунды", () => {
    expect(detectUnixUnit(1_700_000_000)).toBe("seconds");
  });

  test("тринадцатизначные — миллисекунды", () => {
    expect(detectUnixUnit(1_700_000_000_000)).toBe("milliseconds");
  });

  test("знак на выбор единицы не влияет", () => {
    expect(detectUnixUnit(-1_700_000_000_000)).toBe("milliseconds");
    expect(detectUnixUnit(-1_700_000_000)).toBe("seconds");
  });
});

describe("decodeUnix", () => {
  test("секунды разворачиваются в дату", () => {
    const result = decodeUnix("1700000000");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(formatIsoUtc(result.value.date)).toBe("2023-11-14T22:13:20.000Z");
    expect(result.value.unit).toBe("seconds");
    expect(result.value.autoDetected).toBe(true);
    expect(result.value.milliseconds).toBe(1_700_000_000_000);
  });

  test("миллисекунды дают ту же дату, что и секунды", () => {
    const seconds = decodeUnix("1700000000");
    const millis = decodeUnix("1700000000000");
    expect(seconds.ok && millis.ok).toBe(true);
    if (!seconds.ok || !millis.ok) return;
    expect(millis.value.unit).toBe("milliseconds");
    expect(millis.value.date.getTime()).toBe(seconds.value.date.getTime());
  });

  test("явная единица перебивает автоопределение", () => {
    const result = decodeUnix("1700000000", "milliseconds");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value.unit).toBe("milliseconds");
    expect(result.value.autoDetected).toBe(false);
    // 1 700 000 000 мс — это всего 19 суток от начала эпохи, а не 2023 год.
    expect(formatIsoUtc(result.value.date)).toBe("1970-01-20T16:13:20.000Z");
  });

  test("ноль — это начало эпохи", () => {
    const result = decodeUnix("0");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(formatIsoUtc(result.value.date)).toBe("1970-01-01T00:00:00.000Z");
  });

  test("отрицательные метки — даты до 1970 года", () => {
    const result = decodeUnix("-86400");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(formatIsoUtc(result.value.date)).toBe("1969-12-31T00:00:00.000Z");
  });

  test("дробные секунды сохраняют миллисекунды", () => {
    const result = decodeUnix("1700000000.5");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value.milliseconds).toBe(1_700_000_000_500);
    expect(formatIsoUtc(result.value.date)).toBe("2023-11-14T22:13:20.500Z");
  });

  test("пробелы и разделители разрядов игнорируются", () => {
    const result = decodeUnix(" 1 700 000 000 ");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value.milliseconds).toBe(1_700_000_000_000);
  });

  test("пустая строка — ошибка, а не ноль", () => {
    const result = decodeUnix("   ");
    expect(result.ok).toBe(false);
    expect(reasonOf(result)).toBe("Введите Unix-время");
  });

  test("нечисловой ввод отбивается", () => {
    expect(decodeUnix("сегодня").ok).toBe(false);
    expect(decodeUnix("12ab").ok).toBe(false);
    expect(decodeUnix("0x1f").ok).toBe(false);
  });

  test("метка за пределами Date отбивается, а не даёт Invalid Date", () => {
    const result = decodeUnix("99999999999999999999");
    expect(result.ok).toBe(false);
    expect(reasonOf(result)).toBe("Дата вне диапазона, поддерживаемого JavaScript");
  });
});

describe("partsToUnix", () => {
  const noon: Parameters<typeof partsToUnix>[0] = {
    year: 2023,
    month: 11,
    day: 14,
    hour: 22,
    minute: 13,
    second: 20,
    millisecond: 0,
  };

  test("UTC-сборка даёт ровно ту метку, из которой дата и получена", () => {
    const result = partsToUnix(noon, "utc");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value.seconds).toBe(1_700_000_000);
    expect(result.value.milliseconds).toBe(1_700_000_000_000);
  });

  test("локальная сборка совпадает со смещением зоны", () => {
    const result = partsToUnix(noon, "local");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const offsetMs = result.value.date.getTimezoneOffset() * 60_000;
    expect(result.value.milliseconds).toBe(1_700_000_000_000 + offsetMs);
  });

  test("несуществующая дата отбивается, а не переносится на следующий месяц", () => {
    const result = partsToUnix({ ...noon, month: 2, day: 31 }, "utc");
    expect(result.ok).toBe(false);
    expect(reasonOf(result)).toBe("Такой даты не существует");
  });

  test("29 февраля проходит в високосный год и падает в обычный", () => {
    expect(partsToUnix({ ...noon, year: 2024, month: 2, day: 29 }, "utc").ok).toBe(true);
    expect(partsToUnix({ ...noon, year: 2023, month: 2, day: 29 }, "utc").ok).toBe(false);
  });

  test("выход за границы поля называет само поле", () => {
    expect(reasonOf(partsToUnix({ ...noon, month: 13 }, "utc"))).toBe("Месяц: допустимо 1–12");
    expect(reasonOf(partsToUnix({ ...noon, hour: 24 }, "utc"))).toBe("Часы: допустимо 0–23");
    expect(reasonOf(partsToUnix({ ...noon, millisecond: 1000 }, "utc"))).toBe(
      "Миллисекунды: допустимо 0–999",
    );
  });

  test("двузначный год не подменяется на 19xx", () => {
    const result = partsToUnix(
      { year: 50, month: 1, day: 1, hour: 0, minute: 0, second: 0, millisecond: 0 },
      "utc",
    );
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value.date.getUTCFullYear()).toBe(50);
  });

  test("год до нашей эры остаётся отрицательным", () => {
    const result = partsToUnix(
      { year: -44, month: 3, day: 15, hour: 12, minute: 0, second: 0, millisecond: 0 },
      "utc",
    );
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value.date.getUTCFullYear()).toBe(-44);
  });
});

describe("round-trip", () => {
  test("decode → parts → encode возвращает исходную метку", () => {
    const decoded = decodeUnix("1700000000");
    expect(decoded.ok).toBe(true);
    if (!decoded.ok) return;

    for (const zone of ["utc", "local"] as const) {
      const parts = partsFromDate(decoded.value.date, zone);
      const encoded = partsToUnix(parts, zone);
      expect(encoded.ok).toBe(true);
      if (!encoded.ok) return;
      expect(encoded.value.milliseconds).toBe(decoded.value.milliseconds);
    }
  });
});

describe("formatIsoLocal", () => {
  test("оканчивается смещением, а не Z", () => {
    const iso = formatIsoLocal(new Date(1_700_000_000_000));
    expect(iso).toMatch(/[+-]\d{2}:\d{2}$/);
  });

  test("совпадает с UTC-представлением, если сдвинуть на смещение зоны", () => {
    const date = new Date(1_700_000_000_000);
    const shifted = new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
    expect(formatIsoLocal(date).slice(0, 23)).toBe(shifted.toISOString().slice(0, 23));
  });
});

describe("partsFromStrings", () => {
  test("пустое поле не превращается в ноль", () => {
    expect(
      partsFromStrings({
        year: "2023",
        month: "",
        day: "14",
        hour: "0",
        minute: "0",
        second: "0",
        millisecond: "0",
      }),
    ).toBeNull();
  });

  test("дробное значение поля отбивается", () => {
    expect(
      partsFromStrings({
        year: "2023",
        month: "11.5",
        day: "14",
        hour: "0",
        minute: "0",
        second: "0",
        millisecond: "0",
      }),
    ).toBeNull();
  });
});

describe("formatSeconds", () => {
  test("целые секунды печатаются без дробной части", () => {
    expect(formatSeconds(1_700_000_000)).toBe("1700000000");
  });

  test("дробные — с миллисекундами", () => {
    expect(formatSeconds(1_700_000_000.5)).toBe("1700000000.500");
  });
});
