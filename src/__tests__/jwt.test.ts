import { describe, expect, test } from "bun:test";
import {
  formatJwtClaimValue,
  jwtClaimEntries,
  jwtSummary,
  parseJwt,
} from "../lib/jwt";

const SAMPLE =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2HT4EpwuHnKz-zZX0";

describe("jwt", () => {
  test("parseJwt разбирает header, payload и signature", () => {
    const result = parseJwt(SAMPLE);
    expect(result.ok).toBe(true);
    if (!result.ok) return;

    expect(result.value.header).toEqual({ alg: "HS256", typ: "JWT" });
    expect(result.value.payload.sub).toBe("1234567890");
    expect(result.value.payload.name).toBe("John Doe");
    expect(result.value.payload.iat).toBe(1516239022);
    expect(result.value.signature).toBe("SflKxwRJSMeKKF2HT4EpwuHnKz-zZX0");
  });

  test("parseJwt принимает префикс Bearer", () => {
    const result = parseJwt(`Bearer ${SAMPLE}`);
    expect(result.ok).toBe(true);
  });

  test("невалидный токен возвращает понятную ошибку", () => {
    expect(parseJwt("one.two").ok).toBe(false);
    expect(parseJwt("").ok).toBe(false);
  });

  test("formatJwtClaimValue добавляет дату для iat/exp/nbf", () => {
    const formatted = formatJwtClaimValue("iat", 1516239022);
    expect(formatted.startsWith("1516239022 (")).toBe(true);
  });

  test("jwtSummary и jwtClaimEntries собирают метаданные", () => {
    const parsed = parseJwt(SAMPLE);
    if (!parsed.ok) throw new Error("expected parsed jwt");

    expect(jwtSummary(parsed.value)).toEqual({ alg: "HS256", typ: "JWT" });
    expect(jwtClaimEntries(parsed.value.payload).some((entry) => entry.key === "sub")).toBe(true);
  });
});
