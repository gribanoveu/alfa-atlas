/**
 * Чистые преобразования для конвертера Unixtime: разбор введённого числа в
 * дату и обратная сборка даты из полей в Unix-время. Никакого React и
 * никакого IPC — только Date и Intl, поэтому всё покрывается `bun test`.
 */

/** Единица, в которой задано число: Unix timestamp (сек) или Timestamp (мс). */
export type UnixUnit = "seconds" | "milliseconds";

/** Как трактовать введённое число: определить по величине или взять явно. */
export type UnixUnitMode = UnixUnit | "auto";

/** Часовой пояс, в котором пользователь собирает дату руками. */
export type DateZone = "local" | "utc";

export type UnixDecoded = {
  date: Date;
  /** Единица, которая в итоге применена к числу. */
  unit: UnixUnit;
  /** true — единицу выбрали за пользователя, по величине числа. */
  autoDetected: boolean;
  /** Нормализованное значение в секундах (дробное, если во вводе были мс). */
  seconds: number;
  milliseconds: number;
};

export type DecodeResult =
  | { ok: true; value: UnixDecoded }
  | { ok: false; reason: string };

export type DateParts = {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
  second: number;
  millisecond: number;
};

export type EncodeResult =
  | { ok: true; value: { date: Date; seconds: number; milliseconds: number } }
  | { ok: false; reason: string };

/** Предел, за которым Date становится Invalid Date (±100 млн суток). */
const MAX_TIME_MS = 8.64e15;

/**
 * Граница «секунды или миллисекунды». 1e11 секунд — это 5138 год, а 1e11
 * миллисекунд — 1973-й: числа крупнее почти наверняка миллисекунды, мельче —
 * секунды. Промах возможен только на миллисекундных метках 1970–1973 годов,
 * и на этот случай единицу можно задать явно.
 */
const MILLISECONDS_THRESHOLD = 1e11;

export function detectUnixUnit(value: number): UnixUnit {
  return Math.abs(value) >= MILLISECONDS_THRESHOLD ? "milliseconds" : "seconds";
}

/**
 * Разбирает введённое пользователем число в дату. Пробелы и разделители
 * разрядов игнорируются, знак и дробная часть допускаются (метки до 1970-го
 * отрицательны, а секунды бывают дробными).
 */
export function decodeUnix(raw: string, mode: UnixUnitMode = "auto"): DecodeResult {
  const trimmed = raw.trim().replace(/[\s_']/g, "");
  if (!trimmed) return { ok: false, reason: "Введите Unix-время" };
  if (!/^[+-]?\d+(\.\d+)?$/.test(trimmed)) {
    return { ok: false, reason: "Ожидается число: только цифры, знак и точка" };
  }

  const value = Number(trimmed);
  if (!Number.isFinite(value)) {
    return { ok: false, reason: "Число слишком велико" };
  }

  const unit = mode === "auto" ? detectUnixUnit(value) : mode;
  const milliseconds = Math.round(unit === "seconds" ? value * 1000 : value);
  if (!Number.isFinite(milliseconds) || Math.abs(milliseconds) > MAX_TIME_MS) {
    return { ok: false, reason: "Дата вне диапазона, поддерживаемого JavaScript" };
  }

  const date = new Date(milliseconds);
  if (Number.isNaN(date.getTime())) {
    return { ok: false, reason: "Не удалось разобрать дату" };
  }

  return {
    ok: true,
    value: {
      date,
      unit,
      autoDetected: mode === "auto",
      seconds: milliseconds / 1000,
      milliseconds,
    },
  };
}

function pad(value: number, width = 2): string {
  return String(Math.abs(value)).padStart(width, "0");
}

/** Год со знаком: ISO 8601 требует минимум четыре цифры. */
function padYear(year: number): string {
  return (year < 0 ? "-" : "") + pad(year, 4);
}

/** ISO 8601 в UTC — то же, что `Date.prototype.toISOString`. */
export function formatIsoUtc(date: Date): string {
  return date.toISOString();
}

/** ISO 8601 в локальной зоне, со смещением вместо `Z`. */
export function formatIsoLocal(date: Date): string {
  const offsetMinutes = -date.getTimezoneOffset();
  const sign = offsetMinutes < 0 ? "-" : "+";
  const offset = `${sign}${pad(Math.trunc(Math.abs(offsetMinutes) / 60))}:${pad(
    Math.abs(offsetMinutes) % 60,
  )}`;
  return (
    `${padYear(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    `T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}` +
    `.${pad(date.getMilliseconds(), 3)}${offset}`
  );
}

/** Локальная строка в текущей локали среды — `Date.prototype.toLocaleString`. */
export function formatLocale(date: Date): string {
  return date.toLocaleString();
}

/** Та же локальная строка, но принудительно в UTC. */
export function formatLocaleUtc(date: Date): string {
  return date.toLocaleString(undefined, { timeZone: "UTC" });
}

/** Имя часового пояса среды, например `Europe/Moscow`. */
export function localTimeZoneName(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "локальная зона";
  } catch {
    return "локальная зона";
  }
}

/** Целые секунды — то, что обычно и называют Unix timestamp. */
export function formatSeconds(seconds: number): string {
  return Number.isInteger(seconds) ? String(seconds) : seconds.toFixed(3);
}

/** Разбирает дату из полей формы. Пустое поле — не ноль, а ошибка ввода. */
export function partsFromStrings(input: {
  year: string;
  month: string;
  day: string;
  hour: string;
  minute: string;
  second: string;
  millisecond: string;
}): DateParts | null {
  const parsed = {
    year: Number(input.year),
    month: Number(input.month),
    day: Number(input.day),
    hour: Number(input.hour),
    minute: Number(input.minute),
    second: Number(input.second),
    millisecond: Number(input.millisecond),
  };
  const raw = Object.values(input);
  if (raw.some((value) => value.trim() === "")) return null;
  if (Object.values(parsed).some((value) => !Number.isInteger(value))) return null;
  return parsed;
}

const FIELD_RANGES: {
  key: keyof DateParts;
  min: number;
  max: number;
  label: string;
}[] = [
  { key: "month", min: 1, max: 12, label: "Месяц" },
  { key: "day", min: 1, max: 31, label: "День" },
  { key: "hour", min: 0, max: 23, label: "Часы" },
  { key: "minute", min: 0, max: 59, label: "Минуты" },
  { key: "second", min: 0, max: 59, label: "Секунды" },
  { key: "millisecond", min: 0, max: 999, label: "Миллисекунды" },
];

/**
 * Собирает дату из полей и переводит её в Unix-время.
 *
 * Компоненты выставляются через `setFullYear`/`setUTCFullYear`, а не через
 * конструктор `new Date(y, m, ...)`: тот трактует годы 0–99 как 1900+y, из-за
 * чего 0050 год молча превратился бы в 1950-й.
 */
export function partsToUnix(parts: DateParts, zone: DateZone): EncodeResult {
  for (const { key, min, max, label } of FIELD_RANGES) {
    const value = parts[key];
    if (value < min || value > max) {
      return { ok: false, reason: `${label}: допустимо ${min}–${max}` };
    }
  }

  const date = new Date(0);
  if (zone === "utc") {
    date.setUTCFullYear(parts.year, parts.month - 1, parts.day);
    date.setUTCHours(parts.hour, parts.minute, parts.second, parts.millisecond);
  } else {
    date.setFullYear(parts.year, parts.month - 1, parts.day);
    date.setHours(parts.hour, parts.minute, parts.second, parts.millisecond);
  }

  const milliseconds = date.getTime();
  if (Number.isNaN(milliseconds) || Math.abs(milliseconds) > MAX_TIME_MS) {
    return { ok: false, reason: "Дата вне диапазона, поддерживаемого JavaScript" };
  }

  // Date молча переносит лишние дни (31 февраля → 3 марта). Сверяем календарную
  // часть с введённой: если её сдвинуло — такой даты не существует. Время не
  // проверяем: в час перехода на летнее время сдвиг законен.
  const backYear = zone === "utc" ? date.getUTCFullYear() : date.getFullYear();
  const backMonth = (zone === "utc" ? date.getUTCMonth() : date.getMonth()) + 1;
  const backDay = zone === "utc" ? date.getUTCDate() : date.getDate();
  if (backYear !== parts.year || backMonth !== parts.month || backDay !== parts.day) {
    return { ok: false, reason: "Такой даты не существует" };
  }

  return {
    ok: true,
    value: { date, seconds: milliseconds / 1000, milliseconds },
  };
}

/** Раскладывает дату на поля формы — для кнопки «Сейчас». */
export function partsFromDate(date: Date, zone: DateZone): DateParts {
  return zone === "utc"
    ? {
        year: date.getUTCFullYear(),
        month: date.getUTCMonth() + 1,
        day: date.getUTCDate(),
        hour: date.getUTCHours(),
        minute: date.getUTCMinutes(),
        second: date.getUTCSeconds(),
        millisecond: date.getUTCMilliseconds(),
      }
    : {
        year: date.getFullYear(),
        month: date.getMonth() + 1,
        day: date.getDate(),
        hour: date.getHours(),
        minute: date.getMinutes(),
        second: date.getSeconds(),
        millisecond: date.getMilliseconds(),
      };
}
