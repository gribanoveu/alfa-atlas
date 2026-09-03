import { describe, expect, test } from "bun:test";
import {
  BODY_HEADERS,
  CORPORATE_HEADERS,
  corporateHeadersFor,
  describeHttpRequest,
  emptyParam,
  ensureHeaders,
  inferParamsFromJson,
  jsonFormatOf,
  mergeInferredParams,
  missingPathParams,
} from "../lib/httpRequestSpec";
import { emptyArtifactContent, type ParamSpec } from "../lib/artifacts";

describe("jsonFormatOf", () => {
  test("names the JSON type, separating integers from other numbers", () => {
    expect(jsonFormatOf("x")).toBe("string");
    expect(jsonFormatOf(3)).toBe("integer");
    expect(jsonFormatOf(3.5)).toBe("number");
    expect(jsonFormatOf(true)).toBe("boolean");
    expect(jsonFormatOf(null)).toBe("null");
    expect(jsonFormatOf({})).toBe("object");
  });

  test("reports an array's element type when it is uniform", () => {
    expect(jsonFormatOf(["a", "b"])).toBe("array<string>");
    expect(jsonFormatOf([{ a: 1 }])).toBe("array<object>");
    expect(jsonFormatOf(["a", 1])).toBe("array");
    expect(jsonFormatOf([])).toBe("array");
  });
});

describe("inferParamsFromJson", () => {
  test("produces one dotted row per field, with format and example", () => {
    const rows = inferParamsFromJson('{"type":"INVOICE","amount":100,"payer":{"id":"U1"}}');
    expect(rows.map((r) => r.name)).toEqual(["type", "amount", "payer", "payer.id"]);
    expect(rows[0]).toEqual({
      name: "type",
      format: "string",
      required: true,
      description: "",
      values: "INVOICE",
    });
    expect(rows[1]!.format).toBe("integer");
    expect(rows[2]!.format).toBe("object");
    // Objects contribute no example of their own — their fields are rows.
    expect(rows[2]!.values).toBe("");
  });

  test("documents an array's element fields under the array's own name", () => {
    // `items[0].id` is an artifact of the example; `items.id` is the field.
    const rows = inferParamsFromJson('{"items":[{"id":"D1","sum":2}]}');
    expect(rows.map((r) => r.name)).toEqual(["items", "items.id", "items.sum"]);
    expect(rows[0]!.format).toBe("array<object>");
  });

  test("unwraps a top-level array to its element's fields", () => {
    expect(inferParamsFromJson('[{"id":"D1"}]').map((r) => r.name)).toEqual(["id"]);
  });

  test("leaves descriptions empty — meaning is the part only the user knows", () => {
    expect(inferParamsFromJson('{"a":1}').every((r) => r.description === "")).toBe(true);
  });

  test("returns nothing for unparseable or scalar input rather than throwing", () => {
    expect(inferParamsFromJson("not json")).toEqual([]);
    expect(inferParamsFromJson("")).toEqual([]);
    expect(inferParamsFromJson('"a string"')).toEqual([]);
  });

  test("stops descending at a depth cap so a cyclic-looking example terminates", () => {
    const deep = '{"a":{"b":{"c":{"d":{"e":{"f":1}}}}}}';
    const names = inferParamsFromJson(deep).map((r) => r.name);
    expect(names).toContain("a.b.c.d");
    expect(names.some((n) => n.split(".").length > 6)).toBe(false);
  });
});

describe("mergeInferredParams", () => {
  const described: ParamSpec = {
    name: "type",
    format: "",
    required: false,
    description: "Тип документа",
    values: "",
  };

  test("keeps what the user typed and only fills in what was missing", () => {
    const merged = mergeInferredParams(
      [described],
      inferParamsFromJson('{"type":"INVOICE"}'),
    );
    expect(merged).toHaveLength(1);
    expect(merged[0]!.description).toBe("Тип документа");
    // `required: false` was a deliberate choice, not an absence.
    expect(merged[0]!.required).toBe(false);
    expect(merged[0]!.format).toBe("string");
    expect(merged[0]!.values).toBe("INVOICE");
  });

  test("keeps rows absent from the sample — one example is not the whole contract", () => {
    const merged = mergeInferredParams(
      [described, { ...emptyParam(), name: "comment" }],
      inferParamsFromJson('{"type":"INVOICE"}'),
    );
    expect(merged.map((r) => r.name)).toEqual(["type", "comment"]);
  });

  test("adds rows the sample introduced", () => {
    const merged = mergeInferredParams([described], inferParamsFromJson('{"type":"X","sum":1}'));
    expect(merged.map((r) => r.name)).toEqual(["type", "sum"]);
  });

  test("ignores blank-named rows rather than matching them against each other", () => {
    const merged = mergeInferredParams([emptyParam()], inferParamsFromJson('{"a":1}'));
    expect(merged.map((r) => r.name)).toEqual(["a", ""]);
  });
});

describe("missingPathParams", () => {
  test("lists placeholders in the path that have no row yet", () => {
    const spec = {
      ...emptyArtifactContent("httpRequest"),
      path: "/api/{organizationId}/documents/{documentId}",
      pathParams: [{ ...emptyParam(), name: "organizationId" }],
    };
    expect(missingPathParams(spec)).toEqual(["documentId"]);
  });

  test("does not report a placeholder twice", () => {
    const spec = {
      ...emptyArtifactContent("httpRequest"),
      path: "/a/{id}/b/{id}",
    };
    expect(missingPathParams(spec)).toEqual(["id"]);
  });

  test("is empty when the path has no placeholders", () => {
    expect(missingPathParams({ ...emptyArtifactContent("httpRequest"), path: "/ping" })).toEqual([]);
  });
});

describe("describeHttpRequest", () => {
  test("joins method and path, tolerating either being blank", () => {
    const base = emptyArtifactContent("httpRequest");
    expect(describeHttpRequest({ ...base, method: "post", path: "/a" })).toBe("POST /a");
    expect(describeHttpRequest({ ...base, method: "", path: "/a" })).toBe("/a");
    expect(describeHttpRequest({ ...base, method: "get", path: "" })).toBe("GET");
    expect(describeHttpRequest({ ...base, method: "", path: "" })).toBe("");
  });
});

function headerRow(name: string, overrides: Partial<ParamSpec> = {}): ParamSpec {
  return { name, format: "string", required: true, description: "", values: "", ...overrides };
}

describe("CORPORATE_HEADERS", () => {
  test("carries the house block in order, with A-userIp the only optional one", () => {
    expect(CORPORATE_HEADERS.map((h) => [h.name, h.required, h.values])).toEqual([
      ["A-userId", true, "XAAAAA"],
      ["A-customerId", true, "UAAAAA"],
      ["A-projectId", true, "CORP-"],
      ["A-clientType", true, "FRONT"],
      ["A-channelId", true, "NIB"],
      ["A-userIp", false, "192.168.0.0"],
    ]);
  });
});

describe("ensureHeaders", () => {
  test("appends only what is missing", () => {
    expect(ensureHeaders([headerRow("A-userId")], CORPORATE_HEADERS).map((h) => h.name)).toEqual([
      "A-userId",
      "A-customerId",
      "A-projectId",
      "A-clientType",
      "A-channelId",
      "A-userIp",
    ]);
  });

  test("never overwrites a row the user already filled in", () => {
    const mine = headerRow("A-projectId", { description: "мой проект", values: "CORP-42" });
    expect(ensureHeaders([mine], CORPORATE_HEADERS)).toContainEqual(mine);
  });

  test("matches header names case-insensitively", () => {
    const next = ensureHeaders([headerRow("content-type", { values: "text/plain" })], BODY_HEADERS);
    expect(next.map((h) => h.name)).toEqual(["content-type", "Accept"]);
  });

  test("returns the array untouched when nothing is missing", () => {
    const existing = CORPORATE_HEADERS.map((h) => ({ ...h }));
    expect(ensureHeaders(existing, CORPORATE_HEADERS)).toBe(existing);
  });

  test("inserts copies — editing a row must not mutate the catalogue", () => {
    const inserted = ensureHeaders([], CORPORATE_HEADERS);
    inserted[0]!.description = "изменено";
    expect(CORPORATE_HEADERS[0]!.description).toBe("xPin клиента");
  });
});

describe("corporateHeadersFor", () => {
  test("adds the body pair only when the request has a body", () => {
    expect(corporateHeadersFor(false).map((h) => h.name)).not.toContain("Content-Type");
    expect(corporateHeadersFor(true).map((h) => h.name)).toEqual([
      ...CORPORATE_HEADERS.map((h) => h.name),
      "Content-Type",
      "Accept",
    ]);
  });
});
