import { describe, expect, test } from "bun:test";
import { findHttpStatusCode, HTTP_STATUS_CODES } from "../data/httpStatusCodes";
import {
  countHttpStatusMatches,
  filterHttpStatusGroups,
} from "../lib/httpStatusCodes";

describe("httpStatusCodes", () => {
  test("справочник содержит все переданные коды", () => {
    expect(HTTP_STATUS_CODES.length).toBe(63);
    expect(findHttpStatusCode(404)?.name).toBe("Not Found");
    expect(findHttpStatusCode(418)?.name).toBe("I'm a teapot");
  });

  test("filterHttpStatusGroups ищет по коду, названию, описанию и совету", () => {
    expect(filterHttpStatusGroups("Not Found")[0]?.codes[0]?.code).toBe(404);
    expect(filterHttpStatusGroups("teapot")[0]?.codes[0]?.code).toBe(418);
    expect(filterHttpStatusGroups("шлюз")[0]?.codes.some((entry) => entry.code === 502)).toBe(
      true,
    );
    expect(findHttpStatusCode(404)?.usage).toContain("id");
  });

  test("фильтр по классу ограничивает группы", () => {
    const groups = filterHttpStatusGroups("", "5xx");
    expect(groups).toHaveLength(1);
    expect(groups[0]?.id).toBe("5xx");
    expect(countHttpStatusMatches("", "5xx")).toBe(11);
  });
});
