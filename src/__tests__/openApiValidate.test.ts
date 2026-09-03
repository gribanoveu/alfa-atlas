import { describe, expect, test } from "bun:test";
import {
  effectiveParameters,
  effectiveServers,
  namedExamples,
} from "../components/OpenApiExplorer/openApiModel";

describe("effectiveParameters", () => {
  const document = {
    paths: {
      "/pets/{id}": {
        parameters: [
          { name: "id", in: "path", required: true, schema: { type: "string" } },
          { name: "trace", in: "header", schema: { type: "string" } },
        ],
        get: {
          parameters: [
            { name: "trace", in: "header", required: true, schema: { type: "string" } },
          ],
        },
      },
    },
  };

  test("merges path-level parameters, with the operation overriding by name and location", () => {
    const merged = effectiveParameters(
      document,
      "/pets/{id}",
      document.paths["/pets/{id}"].get,
    );
    expect(merged.map((p) => `${p.in}:${p.name}`)).toEqual(["path:id", "header:trace"]);
    expect(merged.find((p) => p.name === "trace")?.required).toBe(true);
  });
});

describe("effectiveServers", () => {
  const document = {
    servers: [{ url: "https://root" }],
    paths: {
      "/a": {
        servers: [{ url: "https://path-level" }],
        get: { servers: [{ url: "https://operation", description: "своя площадка" }] },
        post: {},
      },
      "/b": { get: {} },
    },
  };

  test("the narrowest declared level wins", () => {
    expect(effectiveServers(document, "/a", document.paths["/a"].get)).toEqual([
      { url: "https://operation", description: "своя площадка" },
    ]);
    expect(effectiveServers(document, "/a", document.paths["/a"].post)).toEqual([
      { url: "https://path-level", description: null },
    ]);
    expect(effectiveServers(document, "/b", document.paths["/b"].get)).toEqual([
      { url: "https://root", description: null },
    ]);
  });
});

describe("namedExamples", () => {
  test("reads named examples and skips entries without a value", () => {
    expect(
      namedExamples({
        examples: {
          empty: { summary: "Пустой список", value: { items: [] } },
          broken: { summary: "нет value" },
        },
      }),
    ).toEqual([
      {
        name: "empty",
        summary: "Пустой список",
        description: null,
        value: { items: [] },
      },
    ]);
  });
});
