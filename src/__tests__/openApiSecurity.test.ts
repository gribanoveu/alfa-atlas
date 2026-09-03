import { describe, expect, test } from "bun:test";
import {
  collectSecuritySchemes,
  credentialsFor,
  resolveOperationSecurity,
  type AuthValues,
} from "../components/OpenApiExplorer/security";
import { buildRequest } from "../components/OpenApiExplorer/requestBuilder";

const document = {
  security: [{ bearerAuth: [] }],
  components: {
    securitySchemes: {
      bearerAuth: { type: "http", scheme: "bearer", bearerFormat: "JWT" },
      basicAuth: { type: "http", scheme: "basic" },
      apiKeyHeader: { type: "apiKey", in: "header", name: "X-Api-Key" },
      apiKeyQuery: { type: "apiKey", in: "query", name: "api_key" },
      sessionCookie: { type: "apiKey", in: "cookie", name: "SESSION" },
      oauth: { type: "oauth2", flows: {} },
      mtls: { type: "mutualTLS" },
    },
  },
};

const schemes = collectSecuritySchemes(document);

describe("collectSecuritySchemes", () => {
  test("classifies every scheme type the viewer can fill", () => {
    expect(schemes.map((s) => [s.id, s.kind])).toEqual([
      ["bearerAuth", "bearer"],
      ["basicAuth", "basic"],
      ["apiKeyHeader", "apiKey"],
      ["apiKeyQuery", "apiKey"],
      ["sessionCookie", "apiKey"],
      ["oauth", "oauth2"],
      ["mtls", "unsupported"],
    ]);
  });
});

describe("resolveOperationSecurity", () => {
  test("falls back to the document-level requirement", () => {
    expect(resolveOperationSecurity(document, {})).toEqual({
      schemeIds: ["bearerAuth"],
      optional: false,
      declared: true,
    });
  });

  test("an operation's own requirement replaces the global one", () => {
    expect(
      resolveOperationSecurity(document, { security: [{ apiKeyHeader: [] }] }).schemeIds,
    ).toEqual(["apiKeyHeader"]);
  });

  test("an empty array opts the operation out of the global requirement", () => {
    expect(resolveOperationSecurity(document, { security: [] })).toEqual({
      schemeIds: [],
      optional: true,
      declared: false,
    });
  });

  test("an empty alternative makes auth optional but still declared", () => {
    const security = resolveOperationSecurity(document, {
      security: [{ bearerAuth: [] }, {}],
    });
    expect(security).toEqual({
      schemeIds: ["bearerAuth"],
      optional: true,
      declared: true,
    });
  });

  test("collects every scheme across OR-alternatives", () => {
    expect(
      resolveOperationSecurity(document, {
        security: [{ bearerAuth: [] }, { apiKeyHeader: [], apiKeyQuery: [] }],
      }).schemeIds,
    ).toEqual(["bearerAuth", "apiKeyHeader", "apiKeyQuery"]);
  });
});

describe("credentialsFor", () => {
  const values: AuthValues = {
    bearerAuth: { kind: "token", token: "abc.def" },
    basicAuth: { kind: "basic", username: "u", password: "пароль" },
    apiKeyHeader: { kind: "token", token: "key-1" },
    apiKeyQuery: { kind: "token", token: "key-2" },
    sessionCookie: { kind: "token", token: "sid" },
    oauth: { kind: "token", token: "oauth-token" },
  };

  test("bearer and oauth2 tokens go into Authorization", () => {
    expect(credentialsFor(schemes, values, ["bearerAuth"])).toEqual([
      { in: "header", name: "Authorization", value: "Bearer abc.def" },
    ]);
    expect(credentialsFor(schemes, values, ["oauth"])).toEqual([
      { in: "header", name: "Authorization", value: "Bearer oauth-token" },
    ]);
  });

  test("basic credentials survive a non-ASCII password", () => {
    const [credential] = credentialsFor(schemes, values, ["basicAuth"]);
    expect(credential.name).toBe("Authorization");
    const encoded = credential.value.replace("Basic ", "");
    expect(new TextDecoder().decode(Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0)))).toBe(
      "u:пароль",
    );
  });

  test("apiKey lands in the declared location", () => {
    expect(credentialsFor(schemes, values, ["apiKeyHeader", "apiKeyQuery"])).toEqual([
      { in: "header", name: "X-Api-Key", value: "key-1" },
      { in: "query", name: "api_key", value: "key-2" },
    ]);
  });

  test("cookie schemes are merged into a single Cookie header", () => {
    expect(credentialsFor(schemes, values, ["sessionCookie"])).toEqual([
      { in: "header", name: "Cookie", value: "SESSION=sid" },
    ]);
  });

  test("skips schemes with no value entered and unsupported types", () => {
    expect(credentialsFor(schemes, {}, ["bearerAuth"])).toEqual([]);
    expect(
      credentialsFor(schemes, { mtls: { kind: "token", token: "x" } }, ["mtls"]),
    ).toEqual([]);
  });
});

describe("buildRequest with auth", () => {
  const base = {
    baseUrl: "https://api.example.com",
    path: "/pets",
    method: "get",
    bodyMediaType: null,
    bodyText: "",
    hasBody: false,
  };

  test("applies auth headers and query parameters", () => {
    const request = buildRequest({
      ...base,
      paramValues: {},
      paramEntries: [],
      auth: [
        { in: "header", name: "Authorization", value: "Bearer t" },
        { in: "query", name: "api_key", value: "k" },
      ],
    });
    expect(request.headers).toEqual({ Authorization: "Bearer t" });
    expect(request.url).toBe("https://api.example.com/pets?api_key=k");
  });

  test("an explicitly typed parameter wins over the stored credential", () => {
    const request = buildRequest({
      ...base,
      paramValues: { "header:Authorization": "Bearer typed", "query:api_key": "typed" },
      paramEntries: [
        { name: "Authorization", in: "header" },
        { name: "api_key", in: "query" },
      ],
      auth: [
        { in: "header", name: "Authorization", value: "Bearer stored" },
        { in: "query", name: "api_key", value: "stored" },
      ],
    });
    expect(request.headers.Authorization).toBe("Bearer typed");
    expect(request.url).toBe("https://api.example.com/pets?api_key=typed");
  });
});
