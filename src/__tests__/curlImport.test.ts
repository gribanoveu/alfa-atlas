import { describe, expect, test } from "bun:test";
import { applyCurlImport, parseCurl, tokenizeShell } from "../lib/curlImport";
import type { HttpRequestSpec } from "../lib/artifacts";

function emptySpec(): HttpRequestSpec {
  return {
    method: "GET",
    baseUrl: "https://{host}",
    path: "",
    pathParams: [],
    queryParams: [],
    headers: [],
    body: null,
    responses: [],
    errors: [],
    notes: null,
  };
}

describe("tokenizeShell", () => {
  test("keeps quoted arguments whole and drops line continuations", () => {
    expect(
      tokenizeShell("curl 'https://a/b' \\\n  -H \"X-Trace: 1 2\" \\\n  -d '{\"a\": 1}'"),
    ).toEqual(["curl", "https://a/b", "-H", "X-Trace: 1 2", "-d", '{"a": 1}']);
  });

  test("single quotes protect backslashes, double quotes unescape them", () => {
    expect(tokenizeShell(`a 'c:\\path' "say \\"hi\\""`)).toEqual([
      "a",
      "c:\\path",
      'say "hi"',
    ]);
  });

  test("an empty quoted argument survives as an empty token", () => {
    expect(tokenizeShell("curl -d '' https://a")).toEqual(["curl", "-d", "", "https://a"]);
  });
});

describe("parseCurl", () => {
  test("rejects anything that is not a curl command", () => {
    expect(parseCurl("wget https://example.com")).toBeNull();
    expect(parseCurl("")).toBeNull();
    // curl без адреса заполнять нечем.
    expect(parseCurl("curl -X POST")).toBeNull();
  });

  test("splits the url into origin, path and query", () => {
    const result = parseCurl("curl 'https://api.example.com/svc/v1/docs?page=2&size=50'")!;
    expect(result.method).toBe("GET");
    expect(result.baseUrl).toBe("https://api.example.com");
    expect(result.path).toBe("/svc/v1/docs");
    expect(result.queryParams.map((p) => [p.name, p.values])).toEqual([
      ["page", "2"],
      ["size", "50"],
    ]);
  });

  test("url-encoded query values are decoded", () => {
    const result = parseCurl("curl 'https://a.example.com/x?q=%D0%B0%20%D0%B1'")!;
    expect(result.queryParams[0]!.values).toBe("а б");
  });

  test("infers POST from a body and reads JSON fields as parameters", () => {
    const result = parseCurl(
      `curl 'https://a.example.com/docs' -H 'Content-Type: application/json' -d '{"id": 7, "name": "х"}'`,
    )!;
    expect(result.method).toBe("POST");
    expect(result.body?.mediaType).toBe("application/json");
    expect(result.body?.sample).toBe('{"id": 7, "name": "х"}');
    expect(result.body?.params.map((p) => [p.name, p.format])).toEqual([
      ["id", "integer"],
      ["name", "string"],
    ]);
  });

  test("an explicit method wins over the inferred one", () => {
    expect(parseCurl("curl -X PUT https://a.example.com/x -d 'a=1'")!.method).toBe("PUT");
  });

  test("form-urlencoded bodies become parameter rows", () => {
    const result = parseCurl("curl https://a.example.com/x -d 'a=1&b=два'")!;
    expect(result.body?.mediaType).toBe("application/x-www-form-urlencoded");
    expect(result.body?.params.map((p) => [p.name, p.values])).toEqual([
      ["a", "1"],
      ["b", "два"],
    ]);
  });

  test("-G moves the data into the query string and keeps the method GET", () => {
    const result = parseCurl("curl -G https://a.example.com/x -d 'q=abc' -d 'page=1'")!;
    expect(result.method).toBe("GET");
    expect(result.body).toBeNull();
    expect(result.queryParams.map((p) => p.name)).toEqual(["q", "page"]);
  });

  test("-F fields become a multipart body", () => {
    const result = parseCurl("curl https://a.example.com/x -F 'file=@a.pdf' -F 'kind=scan'")!;
    expect(result.method).toBe("POST");
    expect(result.body?.mediaType).toBe("multipart/form-data");
    expect(result.body?.params.map((p) => p.name)).toEqual(["file", "kind"]);
  });

  test("--json implies a JSON content type", () => {
    const result = parseCurl(`curl https://a.example.com/x --json '{"a":1}'`)!;
    expect(result.headers.map((h) => [h.name, h.values])).toEqual([
      ["Content-Type", "application/json"],
    ]);
    expect(result.body?.mediaType).toBe("application/json");
  });

  test("long options written with = are understood", () => {
    const result = parseCurl(
      "curl --url=https://a.example.com/x --request=DELETE --header='X-Trace: 42'",
    )!;
    expect(result.method).toBe("DELETE");
    expect(result.path).toBe("/x");
    expect(result.headers.map((h) => [h.name, h.values])).toEqual([["X-Trace", "42"]]);
  });

  test("noise flags are skipped without eating the url", () => {
    const result = parseCurl(
      "curl -s -k --compressed -o out.json --max-time 30 https://a.example.com/x",
    )!;
    expect(result.baseUrl).toBe("https://a.example.com");
    expect(result.path).toBe("/x");
  });

  test("-I means HEAD", () => {
    expect(parseCurl("curl -I https://a.example.com/x")!.method).toBe("HEAD");
  });
});

describe("parseCurl · секреты", () => {
  test("keeps the auth scheme but replaces the token", () => {
    const result = parseCurl(
      "curl https://a.example.com/x -H 'Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.secret'",
    )!;
    expect(result.headers[0]!.values).toBe("Bearer <токен>");
  });

  test("masks api keys and cookies whole", () => {
    const result = parseCurl(
      "curl https://a.example.com/x -H 'X-Api-Key: live-key-123' -b 'SESSION=abc'",
    )!;
    expect(result.headers.map((h) => [h.name, h.values])).toEqual([
      ["X-Api-Key", "<значение>"],
      ["Cookie", "<значение>"],
    ]);
  });

  test("-u never carries the password into the artifact", () => {
    const result = parseCurl("curl https://a.example.com/x -u admin:hunter2")!;
    expect(JSON.stringify(result)).not.toContain("hunter2");
    expect(result.headers.map((h) => [h.name, h.values])).toEqual([
      ["Authorization", "Basic <токен>"],
    ]);
  });

  test("ordinary headers keep their real value", () => {
    const result = parseCurl("curl https://a.example.com/x -H 'X-Request-Id: 42'")!;
    expect(result.headers[0]!.values).toBe("42");
  });
});

describe("applyCurlImport", () => {
  const imported = parseCurl(
    `curl -X POST 'https://a.example.com/svc/docs?page=1' -H 'X-Trace: 7' -d '{"id":1}'`,
  )!;

  test("fills an empty form from the command", () => {
    const next = applyCurlImport(emptySpec(), imported);
    expect(next.method).toBe("POST");
    expect(next.baseUrl).toBe("https://a.example.com");
    expect(next.path).toBe("/svc/docs");
    expect(next.queryParams.map((p) => p.name)).toEqual(["page"]);
    // Тело есть — значит к заголовкам добавляется обязательная пара.
    expect(next.headers.map((p) => p.name)).toEqual(["X-Trace", "Content-Type", "Accept"]);
    expect(next.body?.params.map((p) => p.name)).toEqual(["id"]);
  });

  test("a description the user already wrote survives a re-import", () => {
    const spec: HttpRequestSpec = {
      ...emptySpec(),
      queryParams: [
        {
          name: "page",
          format: "integer",
          required: false,
          description: "Номер страницы",
          values: "1",
        },
      ],
    };
    const next = applyCurlImport(spec, imported);
    expect(next.queryParams).toHaveLength(1);
    expect(next.queryParams[0]!.description).toBe("Номер страницы");
    expect(next.queryParams[0]!.format).toBe("integer");
  });

  test("a body sample the user typed is not overwritten", () => {
    const spec: HttpRequestSpec = {
      ...emptySpec(),
      body: { mediaType: "application/json", sample: '{"мой": "пример"}', params: [] },
    };
    const next = applyCurlImport(spec, imported);
    expect(next.body?.sample).toBe('{"мой": "пример"}');
    expect(next.body?.params.map((p) => p.name)).toEqual(["id"]);
  });

  test("sections the command says nothing about are left alone", () => {
    const spec: HttpRequestSpec = {
      ...emptySpec(),
      responses: [{ status: "200", description: "OK", sample: "", params: [] }],
      errors: [{ code: "404", description: "Нет документа" }],
      notes: "Заметка",
    };
    const next = applyCurlImport(spec, imported);
    expect(next.responses).toEqual(spec.responses);
    expect(next.errors).toEqual(spec.errors);
    expect(next.notes).toBe("Заметка");
  });
});

describe("applyCurlImport · заголовки для тела", () => {
  test("adds Content-Type and Accept when the command carries a body", () => {
    const imported = parseCurl(`curl https://a.example.com/x -d '{"id":1}'`)!;
    const next = applyCurlImport(emptySpec(), imported);
    expect(next.headers.map((h) => [h.name, h.values])).toEqual([
      ["Content-Type", "application/json"],
      ["Accept", "application/json"],
    ]);
  });

  test("a Content-Type from the command itself wins over the template row", () => {
    const imported = parseCurl(
      "curl https://a.example.com/x -H 'content-type: application/xml' -d '<a/>'",
    )!;
    const next = applyCurlImport(emptySpec(), imported);
    expect(next.headers.map((h) => [h.name, h.values])).toEqual([
      ["content-type", "application/xml"],
      ["Accept", "application/json"],
    ]);
  });

  test("a bodyless command gets no extra headers", () => {
    const imported = parseCurl("curl https://a.example.com/x")!;
    expect(applyCurlImport(emptySpec(), imported).headers).toEqual([]);
  });
});
