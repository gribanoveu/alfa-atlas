import { asObject, isRefMarker, type JsonValue } from "./openApiModel";

/** Как схема подставляется в запрос. `httpOther` — любая другая HTTP-схема
 * (`digest`, `negotiate`, …): подставляем её название как префикс в
 * `Authorization`, что для чтения/отладки полезнее, чем отказ. `oauth2` и
 * `openIdConnect` сведены к «вставьте готовый access token» — полноценный
 * OAuth-редирект из десктопного вьюера не сделать, а токен у пользователя
 * обычно уже есть. */
export type SecuritySchemeKind =
  | "bearer"
  | "basic"
  | "httpOther"
  | "apiKey"
  | "oauth2"
  | "unsupported";

export type SecurityScheme = {
  /** Ключ в `components.securitySchemes` — им же оперирует `security`. */
  id: string;
  kind: SecuritySchemeKind;
  description: string | null;
  /** `apiKey`: имя параметра. */
  name: string | null;
  /** `apiKey`: куда подставлять. */
  in: "header" | "query" | "cookie" | null;
  /** `http`: значение `scheme` (`bearer`, `basic`, `digest`, …). */
  httpScheme: string | null;
  bearerFormat: string | null;
};

export type AuthValue =
  | { kind: "token"; token: string }
  | { kind: "basic"; username: string; password: string };

/** Введённые секреты, по id схемы. Живут только в памяти вкладки — на диск
 * не пишутся и в проектные настройки не попадают. */
export type AuthValues = Record<string, AuthValue>;

export type OperationSecurity = {
  /** Схемы, которые может потребовать операция (объединение всех альтернатив
   * `security`). Спека допускает OR-набор вариантов, но для подстановки и
   * замочка достаточно плоского списка. */
  schemeIds: string[];
  /** Среди альтернатив есть пустая — запрос допустим и без авторизации. */
  optional: boolean;
  /** У операции (или глобально) объявлена непустая `security`. */
  declared: boolean;
};

export type AppliedCredential = {
  in: "header" | "query";
  name: string;
  value: string;
};

function unique(values: string[]): string[] {
  return [...new Set(values)];
}

function schemeKind(raw: JsonValue): SecuritySchemeKind {
  const type = typeof raw.type === "string" ? raw.type.toLowerCase() : "";
  if (type === "apikey") return "apiKey";
  if (type === "oauth2" || type === "openidconnect") return "oauth2";
  if (type === "http") {
    const scheme = typeof raw.scheme === "string" ? raw.scheme.toLowerCase() : "";
    if (scheme === "bearer") return "bearer";
    if (scheme === "basic") return "basic";
    return "httpOther";
  }
  return "unsupported";
}

export function collectSecuritySchemes(document: JsonValue): SecurityScheme[] {
  const components = asObject(document.components);
  const schemes = components ? asObject(components.securitySchemes) : null;
  if (!schemes) return [];
  const result: SecurityScheme[] = [];
  for (const [id, raw] of Object.entries(schemes)) {
    const obj = asObject(raw);
    if (!obj || isRefMarker(obj)) continue;
    const location = typeof obj.in === "string" ? obj.in.toLowerCase() : null;
    result.push({
      id,
      kind: schemeKind(obj),
      description: typeof obj.description === "string" ? obj.description : null,
      name: typeof obj.name === "string" ? obj.name : null,
      in:
        location === "header" || location === "query" || location === "cookie"
          ? location
          : null,
      httpScheme: typeof obj.scheme === "string" ? obj.scheme : null,
      bearerFormat: typeof obj.bearerFormat === "string" ? obj.bearerFormat : null,
    });
  }
  return result;
}

/** `security` операции перекрывает глобальную целиком (в том числе пустым
 * массивом — это явный отказ от авторизации именно для этой ручки). */
export function resolveOperationSecurity(
  document: JsonValue,
  operation: JsonValue,
): OperationSecurity {
  const raw = Array.isArray(operation.security)
    ? operation.security
    : Array.isArray(document.security)
      ? document.security
      : null;
  if (!raw) return { schemeIds: [], optional: true, declared: false };

  const alternatives = raw.map((entry) => Object.keys(asObject(entry) ?? {}));
  const schemeIds = unique(alternatives.flat());
  return {
    schemeIds,
    optional: alternatives.length === 0 || alternatives.some((a) => a.length === 0),
    declared: schemeIds.length > 0,
  };
}

export function emptyValueFor(scheme: SecurityScheme): AuthValue {
  return scheme.kind === "basic"
    ? { kind: "basic", username: "", password: "" }
    : { kind: "token", token: "" };
}

export function isFilled(value: AuthValue | undefined): boolean {
  if (!value) return false;
  return value.kind === "basic"
    ? value.username !== "" || value.password !== ""
    : value.token !== "";
}

/** `btoa` работает с байтами, а не с юникодом: пароль с кириллицей уронил бы
 * его `InvalidCharacterError`. */
function base64Utf8(input: string): string {
  const bytes = new TextEncoder().encode(input);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function credentialForScheme(
  scheme: SecurityScheme,
  value: AuthValue,
): AppliedCredential | { cookie: string } | null {
  if (!isFilled(value)) return null;

  if (scheme.kind === "basic") {
    if (value.kind !== "basic") return null;
    return {
      in: "header",
      name: "Authorization",
      value: `Basic ${base64Utf8(`${value.username}:${value.password}`)}`,
    };
  }

  const token = value.kind === "token" ? value.token : "";
  if (token === "") return null;

  if (scheme.kind === "bearer" || scheme.kind === "oauth2") {
    return { in: "header", name: "Authorization", value: `Bearer ${token}` };
  }
  if (scheme.kind === "httpOther") {
    const prefix = scheme.httpScheme ?? "";
    const label = prefix ? prefix[0].toUpperCase() + prefix.slice(1) : "";
    return {
      in: "header",
      name: "Authorization",
      value: label ? `${label} ${token}` : token,
    };
  }
  if (scheme.kind === "apiKey" && scheme.name) {
    if (scheme.in === "cookie") return { cookie: `${scheme.name}=${token}` };
    if (scheme.in === "query") return { in: "query", name: scheme.name, value: token };
    // Значение `in` бывает не указано или искажено генератором — заголовок
    // здесь безопаснее query, куда ключ утёк бы в логи и в историю.
    return { in: "header", name: scheme.name, value: token };
  }
  return null;
}

/** Собирает заголовки/query-параметры для схем `schemeIds`, у которых есть
 * заполненное значение. Незаполненные молча пропускаются: запрос всё равно
 * уходит (пользователь увидит 401 от сервера, а не отказ вьюера). */
export function credentialsFor(
  schemes: SecurityScheme[],
  values: AuthValues,
  schemeIds: string[],
): AppliedCredential[] {
  const byId = new Map(schemes.map((s) => [s.id, s]));
  const result: AppliedCredential[] = [];
  const cookiePairs: string[] = [];

  for (const id of unique(schemeIds)) {
    const scheme = byId.get(id);
    const value = values[id];
    if (!scheme || !value) continue;
    const credential = credentialForScheme(scheme, value);
    if (!credential) continue;
    if ("cookie" in credential) cookiePairs.push(credential.cookie);
    else result.push(credential);
  }

  if (cookiePairs.length > 0) {
    result.push({ in: "header", name: "Cookie", value: cookiePairs.join("; ") });
  }
  return result;
}

/** Короткая подпись схемы под полем ввода: чем именно её заполнять. */
export function describeScheme(scheme: SecurityScheme): string {
  switch (scheme.kind) {
    case "bearer":
      return `http · bearer${scheme.bearerFormat ? ` (${scheme.bearerFormat})` : ""} → заголовок Authorization`;
    case "basic":
      return "http · basic → заголовок Authorization";
    case "httpOther":
      return `http · ${scheme.httpScheme ?? "?"} → заголовок Authorization`;
    case "apiKey": {
      const where =
        scheme.in === "query" ? "query-параметр" : scheme.in === "cookie" ? "cookie" : "заголовок";
      return `apiKey → ${where} ${scheme.name ?? "?"}`;
    }
    case "oauth2":
      return "oauth2 · вставьте готовый access token → заголовок Authorization";
    default:
      return "тип схемы не поддерживается вьюером";
  }
}
