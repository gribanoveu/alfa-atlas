import type { BodySpec, HttpRequestSpec, ParamSpec } from "./artifacts";
import {
  BODY_HEADERS,
  emptyParam,
  ensureHeaders,
  inferParamsFromJson,
  mergeInferredParams,
} from "./httpRequestSpec";

/** Что удалось вычитать из команды curl. Только те поля, которые в curl
 *  действительно есть: ответы, коды ошибок, обязательность и смысл полей из
 *  него не выводятся и остаются пользователю. */
export type CurlImport = {
  method: string;
  baseUrl: string;
  path: string;
  queryParams: ParamSpec[];
  headers: ParamSpec[];
  body: BodySpec | null;
};

/**
 * Заголовки, значение которых — секрет, а не документация. Имя заголовка в
 * описании метода нужно, значение — нет: в артефакт оно попало бы вместе с
 * живым токеном со стенда и уехало бы в опубликованный документ.
 */
const CREDENTIAL_HEADERS = new Set([
  "authorization",
  "proxy-authorization",
  "cookie",
  "set-cookie",
  "x-api-key",
  "api-key",
  "apikey",
  "x-auth-token",
  "x-access-token",
  "x-csrf-token",
]);

/** Схемы авторизации: саму схему сохраняем — она часть контракта, — а токен
 *  за ней заменяем плейсхолдером. */
const AUTH_SCHEMES = ["bearer", "basic", "digest", "negotiate", "token"];

function maskCredential(name: string, value: string): string {
  if (!CREDENTIAL_HEADERS.has(name.toLowerCase())) return value;
  const [scheme] = value.split(/\s+/, 1);
  if (scheme && AUTH_SCHEMES.includes(scheme.toLowerCase())) {
    return `${scheme} <токен>`;
  }
  return "<значение>";
}

/**
 * Разбивает команду на аргументы по правилам оболочки: одинарные кавычки
 * (внутри них не экранируется ничего), двойные кавычки с `\`-экранированием
 * и перенос строки обратным слешем — в таком виде curl обычно и копируют из
 * DevTools или Postman.
 */
export function tokenizeShell(input: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let started = false;
  let quote: '"' | "'" | null = null;

  for (let i = 0; i < input.length; i += 1) {
    const char = input[i]!;

    if (quote === "'") {
      if (char === "'") quote = null;
      else current += char;
      continue;
    }
    if (quote === '"') {
      if (char === "\\" && i + 1 < input.length) {
        const next = input[i + 1]!;
        // В двойных кавычках оболочка снимает слеш только перед этими
        // символами; перед всем остальным он остаётся частью строки.
        if ('"\\$`\n'.includes(next)) {
          if (next !== "\n") current += next;
          i += 1;
          continue;
        }
      }
      if (char === '"') quote = null;
      else current += char;
      continue;
    }

    if (char === "'" || char === '"') {
      quote = char;
      started = true;
      continue;
    }
    if (char === "\\" && input[i + 1] === "\n") {
      i += 1;
      continue;
    }
    if (/\s/.test(char)) {
      if (started) {
        tokens.push(current);
        current = "";
        started = false;
      }
      continue;
    }
    current += char;
    started = true;
  }
  if (started) tokens.push(current);
  return tokens;
}

/** Флаги curl без значения, которые на содержание запроса не влияют. */
const IGNORED_FLAGS = new Set([
  "-s", "--silent", "-S", "--show-error", "-v", "--verbose", "-k", "--insecure",
  "-L", "--location", "-i", "--include", "-f", "--fail", "--compressed",
  "-#", "--progress-bar", "-N", "--no-buffer", "-4", "-6", "-g", "--globoff",
]);

/** Флаги со значением, которое к описанию метода отношения не имеет. */
const IGNORED_WITH_VALUE = new Set([
  "-o", "--output", "-w", "--write-out", "--max-time", "-m", "--connect-timeout",
  "--retry", "--cacert", "--cert", "--key", "-x", "--proxy", "--resolve",
  "-e", "--referer", "--interface",
]);

const DATA_FLAGS = new Set([
  "-d", "--data", "--data-raw", "--data-ascii", "--data-binary", "--data-urlencode",
]);

function paramFrom(name: string, value: string): ParamSpec {
  return { ...emptyParam(), name, values: value };
}

function splitLongOption(token: string): [string, string | null] {
  if (token.startsWith("--") && token.includes("=")) {
    const at = token.indexOf("=");
    return [token.slice(0, at), token.slice(at + 1)];
  }
  return [token, null];
}

function decode(value: string): string {
  try {
    return decodeURIComponent(value.replace(/\+/g, " "));
  } catch {
    return value;
  }
}

function looksLikeUrl(token: string): boolean {
  return /^https?:\/\//i.test(token) || /^[\w.-]+\.[a-z]{2,}(\/|:|$)/i.test(token);
}

function parseUrl(raw: string): { baseUrl: string; path: string; query: [string, string][] } {
  const withScheme = /^https?:\/\//i.test(raw) ? raw : `https://${raw}`;
  let url: URL;
  try {
    url = new URL(withScheme);
  } catch {
    return { baseUrl: "", path: raw, query: [] };
  }
  const query: [string, string][] = [];
  url.searchParams.forEach((value, name) => query.push([name, value]));
  return { baseUrl: url.origin, path: url.pathname, query };
}

function mediaTypeFor(headers: ParamSpec[], data: string): string {
  const declared = headers.find((h) => h.name.toLowerCase() === "content-type");
  if (declared?.values.trim()) return declared.values.split(";")[0]!.trim();
  const trimmed = data.trim();
  if (trimmed.startsWith("{") || trimmed.startsWith("[")) return "application/json";
  if (/^[^=&\s]+=[^&]*(&[^=&\s]+=[^&]*)*$/.test(trimmed)) {
    return "application/x-www-form-urlencoded";
  }
  return "text/plain";
}

function formUrlencodedParams(data: string): ParamSpec[] {
  return data
    .split("&")
    .filter(Boolean)
    .map((pair) => {
      const at = pair.indexOf("=");
      const name = at >= 0 ? pair.slice(0, at) : pair;
      const value = at >= 0 ? pair.slice(at + 1) : "";
      return paramFrom(decode(name), decode(value));
    });
}

/**
 * Разбирает команду curl. `null` — если это не curl: пусть вызывающий скажет
 * об этом прямо, а не заполняет форму мусором.
 *
 * Поддержаны формы, в которых curl реально копируют: `-X/--request`,
 * `-H/--header`, тело в `-d/--data*` и `--json`, `-G` (тело уходит в query),
 * `-F/--form`, `-u/--user`, `-b/--cookie`, `-A/--user-agent`, `-I/--head`,
 * `--url`, длинные опции через `=`, переносы строк обратным слешем.
 */
export function parseCurl(input: string): CurlImport | null {
  const tokens = tokenizeShell(input.trim());
  if (tokens.length === 0 || tokens[0] !== "curl") return null;

  let url: string | null = null;
  let method: string | null = null;
  let head = false;
  let dataToQuery = false;
  const headers: ParamSpec[] = [];
  const dataParts: string[] = [];
  const formFields: ParamSpec[] = [];
  let user: string | null = null;
  let jsonFlagUsed = false;

  for (let i = 1; i < tokens.length; i += 1) {
    const [flag, inlineValue] = splitLongOption(tokens[i]!);
    const take = (): string => {
      if (inlineValue !== null) return inlineValue;
      i += 1;
      return tokens[i] ?? "";
    };

    if (IGNORED_FLAGS.has(flag)) continue;
    if (IGNORED_WITH_VALUE.has(flag)) {
      take();
      continue;
    }

    switch (flag) {
      case "-X":
      case "--request":
        method = take().toUpperCase();
        break;
      case "-I":
      case "--head":
        head = true;
        break;
      case "-G":
      case "--get":
        dataToQuery = true;
        break;
      case "--url":
        url = take();
        break;
      case "-H":
      case "--header": {
        const raw = take();
        const at = raw.indexOf(":");
        if (at <= 0) break;
        const name = raw.slice(0, at).trim();
        const value = raw.slice(at + 1).trim();
        headers.push(paramFrom(name, maskCredential(name, value)));
        break;
      }
      case "--json":
        jsonFlagUsed = true;
        dataParts.push(take());
        break;
      case "-b":
      case "--cookie":
        headers.push(paramFrom("Cookie", maskCredential("cookie", take())));
        break;
      case "-A":
      case "--user-agent":
        headers.push(paramFrom("User-Agent", take()));
        break;
      case "-u":
      case "--user":
        user = take();
        break;
      case "-F":
      case "--form": {
        const raw = take();
        const at = raw.indexOf("=");
        if (at <= 0) break;
        formFields.push(paramFrom(raw.slice(0, at), raw.slice(at + 1)));
        break;
      }
      default:
        if (DATA_FLAGS.has(flag)) {
          dataParts.push(take());
          break;
        }
        if (!flag.startsWith("-") && looksLikeUrl(flag) && url === null) {
          url = flag;
          break;
        }
        // Неизвестный флаг со значением съел бы URL, поэтому значение за ним
        // не глотаем — лучше пропустить флаг, чем потерять адрес.
        break;
    }
  }

  if (url === null) return null;

  const parsed = parseUrl(url);
  const data = dataParts.join("&");

  if (user !== null) {
    // Пароль в артефакт не переносим — в документе нужен факт basic-авторизации.
    headers.push(paramFrom("Authorization", "Basic <токен>"));
  }
  if (jsonFlagUsed && !headers.some((h) => h.name.toLowerCase() === "content-type")) {
    headers.push(paramFrom("Content-Type", "application/json"));
  }

  const queryPairs = [...parsed.query];
  if (dataToQuery && data) {
    for (const pair of data.split("&").filter(Boolean)) {
      const at = pair.indexOf("=");
      queryPairs.push(
        at >= 0 ? [decode(pair.slice(0, at)), decode(pair.slice(at + 1))] : [decode(pair), ""],
      );
    }
  }

  let body: BodySpec | null = null;
  if (formFields.length > 0) {
    body = { mediaType: "multipart/form-data", sample: "", params: formFields };
  } else if (data && !dataToQuery) {
    const mediaType = mediaTypeFor(headers, data);
    body = {
      mediaType,
      sample: data,
      params:
        mediaType === "application/json"
          ? inferParamsFromJson(data)
          : mediaType === "application/x-www-form-urlencoded"
            ? formUrlencodedParams(data)
            : [],
    };
  }

  const resolvedMethod =
    method ?? (head ? "HEAD" : dataToQuery ? "GET" : body !== null ? "POST" : "GET");

  return {
    method: resolvedMethod,
    baseUrl: parsed.baseUrl,
    path: parsed.path,
    queryParams: queryPairs.map(([name, value]) => paramFrom(name, value)),
    headers,
    body,
  };
}

/**
 * Накладывает разбор на текущую форму.
 *
 * Method/URL перезаписываются — ради них импорт и запускают. Строки таблиц
 * сливаются через `mergeInferredParams`: описания и форматы, которые
 * пользователь уже проставил, переживают повторный импорт, а недописанные
 * пустые строки не пропадают. Тело подставляется целиком только если своего
 * ещё нет — иначе берём из импорта поля, но оставляем набранный пример.
 *
 * Если у запроса есть тело, к заголовкам добавляется пара Content-Type/Accept
 * — тем же правилом, что и при добавлении тела руками. Заголовок, пришедший
 * из самой команды, при этом побеждает: `ensureHeaders` не трогает то, что
 * уже есть.
 */
export function applyCurlImport(
  spec: HttpRequestSpec,
  imported: CurlImport,
): HttpRequestSpec {
  const body: BodySpec | null = (() => {
    if (!imported.body) return spec.body;
    if (!spec.body) return imported.body;
    return {
      mediaType: imported.body.mediaType || spec.body.mediaType,
      sample: spec.body.sample.trim() ? spec.body.sample : imported.body.sample,
      params: mergeInferredParams(spec.body.params, imported.body.params),
    };
  })();

  const headers = mergeInferredParams(spec.headers, imported.headers);

  return {
    ...spec,
    method: imported.method,
    baseUrl: imported.baseUrl || spec.baseUrl,
    path: imported.path || spec.path,
    queryParams: mergeInferredParams(spec.queryParams, imported.queryParams),
    headers: body !== null ? ensureHeaders(headers, BODY_HEADERS) : headers,
    body,
  };
}
