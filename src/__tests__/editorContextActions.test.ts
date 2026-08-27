import { describe, expect, test } from "bun:test";
import { REQUEST_FROM_CURL_PROMPT_PREFIX } from "../lib/assistantConfig";
import {
  editorActionContextFromTab,
  matchesBasename,
  resolveEditorContextActions,
} from "../lib/editorContextActions";

function ctx(overrides: Partial<ReturnType<typeof editorActionContextFromTab>> = {}) {
  return {
    path: "api/getUser/request.adoc",
    basename: "request.adoc",
    content: "== Request\n",
    tabKind: "text" as const,
    tabOrigin: "project" as const,
    llmReady: true,
    ...overrides,
  };
}

describe("matchesBasename", () => {
  test("is case-insensitive", () => {
    expect(matchesBasename("Request.adoc", "request.adoc")).toBe(true);
    expect(matchesBasename("REQUEST.ADOC", "request.adoc")).toBe(true);
    expect(matchesBasename("response.adoc", "request.adoc")).toBe(false);
  });
});

describe("resolveEditorContextActions", () => {
  test("offers curl fill for request.adoc project text tabs when llm is ready", () => {
    const actions = resolveEditorContextActions(ctx());
    expect(actions.map((a) => a.id)).toEqual(["request-from-curl"]);
    expect(actions[0]?.label).toBe("Заполнить по примеру curl");
  });

  test("matches request.adoc case-insensitively", () => {
    const actions = resolveEditorContextActions(
      ctx({ path: "api/getUser/Request.adoc", basename: "Request.adoc" }),
    );
    expect(actions.map((a) => a.id)).toEqual(["request-from-curl"]);
  });

  test("returns no actions for response.adoc", () => {
    const actions = resolveEditorContextActions(
      ctx({ path: "api/getUser/response.adoc", basename: "response.adoc" }),
    );
    expect(actions).toEqual([]);
  });

  test("returns no actions when llm is not ready", () => {
    expect(resolveEditorContextActions(ctx({ llmReady: false }))).toEqual([]);
  });

  test("returns no actions for plan or image tabs", () => {
    expect(resolveEditorContextActions(ctx({ tabKind: "plan" }))).toEqual([]);
    expect(resolveEditorContextActions(ctx({ tabKind: "image" }))).toEqual([]);
  });

  test("returns no actions for external files", () => {
    expect(resolveEditorContextActions(ctx({ tabOrigin: "external" }))).toEqual([]);
  });

  test("buildPrompt includes curl, path, and shared prefix", () => {
    const action = resolveEditorContextActions(ctx())[0];
    const prompt = action?.buildPrompt(ctx(), 'curl -X GET "https://example"');
    expect(prompt).toContain(REQUEST_FROM_CURL_PROMPT_PREFIX);
    expect(prompt).toContain('curl -X GET "https://example"');
    expect(prompt).toContain("`api/getUser/request.adoc`");
    expect(prompt).toContain("method.adoc");
  });
});

describe("editorActionContextFromTab", () => {
  test("derives basename from tab path", () => {
    const result = editorActionContextFromTab(
      {
        path: "nested/request.adoc",
        content: "",
        kind: "text",
        origin: "project",
      },
      true,
    );
    expect(result.basename).toBe("request.adoc");
    expect(result.llmReady).toBe(true);
  });
});
