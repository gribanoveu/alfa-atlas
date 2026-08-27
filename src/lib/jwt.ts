/**
 * Разбор JWT без проверки подписи — чистые функции поверх `jwt-decode`.
 */

import { InvalidTokenError, jwtDecode } from "jwt-decode";

export type JwtPart = Record<string, unknown>;

export type JwtParsed = {
  header: JwtPart;
  payload: JwtPart;
  signature: string;
  headerJson: string;
  payloadJson: string;
};

export type ParseJwtResult =
  | { ok: true; value: JwtParsed }
  | { ok: false; reason: string };

export const JWT_TIME_CLAIMS = ["exp", "iat", "nbf"] as const;

export type JwtTimeClaim = (typeof JWT_TIME_CLAIMS)[number];

export function isJwtTimeClaim(key: string): key is JwtTimeClaim {
  return (JWT_TIME_CLAIMS as readonly string[]).includes(key);
}

/** Unix-секунды из claim JWT → локальная строка даты. */
export function formatJwtTimestamp(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString();
}

/** Человекочитаемое значение claim для таблицы результата. */
export function formatJwtClaimValue(key: string, value: unknown): string {
  if (isJwtTimeClaim(key) && typeof value === "number" && Number.isFinite(value)) {
    return `${value} (${formatJwtTimestamp(value)})`;
  }
  if (value === null) return "null";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

export function parseJwt(raw: string): ParseJwtResult {
  const token = raw.trim().replace(/^Bearer\s+/i, "");
  if (!token) {
    return { ok: false, reason: "Вставьте JWT" };
  }

  const parts = token.split(".");
  if (parts.length !== 3) {
    return {
      ok: false,
      reason: "JWT должен состоять из трёх частей, разделённых точкой",
    };
  }
  if (parts.some((part) => !part)) {
    return { ok: false, reason: "Части JWT не должны быть пустыми" };
  }

  try {
    const header = jwtDecode<JwtPart>(token, { header: true });
    const payload = jwtDecode<JwtPart>(token);
    return {
      ok: true,
      value: {
        header,
        payload,
        signature: parts[2]!,
        headerJson: JSON.stringify(header, null, 2),
        payloadJson: JSON.stringify(payload, null, 2),
      },
    };
  } catch (error) {
    if (error instanceof InvalidTokenError) {
      return { ok: false, reason: error.message };
    }
    return { ok: false, reason: "Не удалось разобрать JWT" };
  }
}

export function jwtSummary(parsed: JwtParsed): { alg: string; typ: string } {
  const alg = parsed.header.alg;
  const typ = parsed.header.typ;
  return {
    alg: typeof alg === "string" ? alg : "—",
    typ: typeof typ === "string" ? typ : "—",
  };
}

export function jwtClaimEntries(payload: JwtPart): { key: string; value: string }[] {
  return Object.entries(payload).map(([key, value]) => ({
    key,
    value: formatJwtClaimValue(key, value),
  }));
}
