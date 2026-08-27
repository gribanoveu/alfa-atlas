import { describe, expect, test } from "bun:test";
import {
  editorActionContextFromTab,
  isMethodDescriptionFile,
  matchesBasename,
  METHOD_STANDARDS_CHECK_PROMPT_PREFIX,
  parentFolderName,
  REQUEST_FROM_CURL_PROMPT_PREFIX,
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

describe("isMethodDescriptionFile", () => {
  test("matches when basename equals parent folder name", () => {
    expect(
      isMethodDescriptionFile(ctx({ path: "api/getUser/getUser.adoc", basename: "getUser.adoc" })),
    ).toBe(true);
  });

  test("is case-insensitive for stem and folder", () => {
    expect(
      isMethodDescriptionFile(ctx({ path: "api/GetUser/GetUser.adoc", basename: "GetUser.adoc" })),
    ).toBe(true);
  });

  test("rejects request.adoc and response.adoc", () => {
    expect(
      isMethodDescriptionFile(ctx({ path: "api/getUser/request.adoc", basename: "request.adoc" })),
    ).toBe(false);
    expect(
      isMethodDescriptionFile(ctx({ path: "api/getUser/response.adoc", basename: "response.adoc" })),
    ).toBe(false);
  });

  test("rejects files at docs root and unrelated names", () => {
    expect(isMethodDescriptionFile(ctx({ path: "intro.adoc", basename: "intro.adoc" }))).toBe(false);
    expect(
      isMethodDescriptionFile(ctx({ path: "api/getUser/overview.adoc", basename: "overview.adoc" })),
    ).toBe(false);
  });
});

describe("method standards check action", () => {
  test("offers draft action for method description files", () => {
    const methodCtx = ctx({ path: "api/getUser/getUser.adoc", basename: "getUser.adoc" });
    const actions = resolveEditorContextActions(methodCtx);
    expect(actions.map((a) => a.id)).toEqual(["method-standards-check"]);
    expect(actions[0]?.label).toBe("Проверить по стандарту");
    expect(actions[0]?.delivery).toBe("draft");
    expect(actions[0]?.input).toEqual({ kind: "none" });
  });

  test("buildPrompt references standards check and method folder", () => {
    const methodCtx = ctx({ path: "api/getUser/getUser.adoc", basename: "getUser.adoc" });
    const action = resolveEditorContextActions(methodCtx)[0];
    const prompt = action?.buildPrompt(methodCtx);
    expect(prompt).toContain(METHOD_STANDARDS_CHECK_PROMPT_PREFIX);
    expect(prompt).toContain('kind: "standards"');
    expect(prompt).toContain("исправь");
    expect(prompt).toContain("`api/getUser/getUser.adoc`");
    expect(prompt).toContain("`api/getUser`");
  });
});

describe("parentFolderName", () => {
  test("returns the last path segment", () => {
    expect(parentFolderName("api/getUser/getUser.adoc")).toBe("getUser");
    expect(parentFolderName("getUser.adoc")).toBe(null);
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
