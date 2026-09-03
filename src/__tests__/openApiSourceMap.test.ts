import { describe, expect, test } from "bun:test";
import {
  buildSourceIndex,
  findSpecLine,
  operationPointer,
  searchKeysForSource,
  sourceForPointer,
} from "../components/OpenApiExplorer/sourceMap";

const index = buildSourceIndex([
  { pointer: "", file: "specs/api.yaml", fragment: "" },
  { pointer: "/paths/~1pets/get", file: "specs/operations/listPets.yaml", fragment: "" },
  {
    pointer: "/paths/~1pets/get/responses/200/content/application~1json/schema",
    file: "specs/schemas/all.yaml",
    fragment: "/Pet",
  },
]);

describe("operationPointer", () => {
  test("escapes slashes in the path template", () => {
    expect(operationPointer("/pets/{id}", "get")).toBe("/paths/~1pets~1{id}/get");
  });
});

describe("sourceForPointer", () => {
  test("exact match wins", () => {
    expect(sourceForPointer(index, "/paths/~1pets/get")?.file).toBe(
      "specs/operations/listPets.yaml",
    );
  });

  test("a nested node inherits the closest recorded ancestor", () => {
    expect(sourceForPointer(index, "/paths/~1pets/get/parameters/0")?.file).toBe(
      "specs/operations/listPets.yaml",
    );
    expect(
      sourceForPointer(
        index,
        "/paths/~1pets/get/responses/200/content/application~1json/schema/properties/name",
      )?.file,
    ).toBe("specs/schemas/all.yaml");
  });

  test("anything unmatched falls back to the entry document", () => {
    expect(sourceForPointer(index, "/info/title")?.file).toBe("specs/api.yaml");
  });

  test("a pointer that only shares a prefix textually is not a descendant", () => {
    const shallow = buildSourceIndex([
      { pointer: "", file: "entry.yaml", fragment: "" },
      { pointer: "/paths/~1pet", file: "pet.yaml", fragment: "" },
    ]);
    // `/paths/~1pets` начинается с `/paths/~1pet`, но потомком не является.
    expect(sourceForPointer(shallow, "/paths/~1pets/get")?.file).toBe("entry.yaml");
  });
});

describe("searchKeysForSource", () => {
  test("prefers the fragment's last segment, then operationId, then the path", () => {
    expect(
      searchKeysForSource(
        { pointer: "x", file: "specs/schemas/all.yaml", fragment: "/Pet" },
        { operationId: "listPets", path: "/pets" },
      ),
    ).toEqual(["Pet", "listPets", "/pets"]);
  });

  test("a whole-file reference falls back to the operation's own markers", () => {
    expect(
      searchKeysForSource(
        { pointer: "x", file: "specs/operations/listPets.yaml", fragment: "" },
        { operationId: "listPets", path: "/pets" },
      ),
    ).toEqual(["listPets", "/pets"]);
  });
});

describe("findSpecLine", () => {
  const yaml = [
    "openapi: 3.0.3",
    "paths:",
    "  /pets:",
    "    get:",
    "      operationId: listPets",
    "components:",
    "  schemas:",
    "    Pet:",
    "      type: object",
  ].join("\n");

  test("finds a map key", () => {
    expect(findSpecLine(yaml, ["Pet"])).toBe(8);
  });

  test("falls through to a plain substring when there is no such key", () => {
    expect(findSpecLine(yaml, ["listPets"])).toBe(5);
  });

  test("honours key order — the first match wins", () => {
    expect(findSpecLine(yaml, ["Pet", "listPets"])).toBe(8);
    expect(findSpecLine(yaml, ["nope", "listPets"])).toBe(5);
  });

  test("quoted keys still match", () => {
    expect(findSpecLine("'200':\n  description: ok\n", ["200"])).toBe(1);
  });

  test("nothing found or nothing asked opens the file at the top", () => {
    expect(findSpecLine(yaml, ["missing"])).toBe(1);
    expect(findSpecLine(yaml, [])).toBe(1);
  });
});
