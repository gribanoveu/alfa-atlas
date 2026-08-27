import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import { useAssistantBridge } from "../hooks/useAssistantBridge";

let renameReports: unknown[] = [];

function makeDeps(activeTabId: string | null = "a.adoc", hasBundle = false) {
  return {
    project: { repoRoot: "/repo", docsRoot: "/repo/docs" },
    editor: {
      activeTabId,
      reloadTabFromDisk: mock(async () => {}),
      discardTabsUnder: mock(() => {}),
      remapTabsUnder: mock(() => {}),
    },
    tree: { refresh: mock(async () => {}) },
    session: { remapExpandedUnder: mock(() => {}), ensureExpanded: mock(() => {}) },
    git: { scheduleRefresh: mock(() => {}) },
    layout: { setRightTool: mock(() => {}) },
    openApiBundle: { bundle: hasBundle ? {} : null, reload: mock(async () => {}) },
    applyRenameReport: mock(async (r: unknown) => {
      renameReports.push(r);
    }),
  };
}

function render(deps: ReturnType<typeof makeDeps>) {
  return renderHook(() => useAssistantBridge(deps as never));
}

beforeEach(() => {
  renameReports = [];
});

describe("useAssistantBridge — the assistant changed files", () => {
  test("a write reloads the tab showing that file", () => {
    const deps = makeDeps();
    const { result } = render(deps);

    act(() => result.current.onFileWritten({ tool: "writeFile", path: "docs/a.adoc" }));

    // Tool results are access-mode-relative; tabs are docs-relative.
    expect(deps.editor.reloadTabFromDisk).toHaveBeenCalledWith("a.adoc");
    expect(deps.tree.refresh).toHaveBeenCalled();
  });

  test("an edit reloads the same way a write does", () => {
    const deps = makeDeps();
    const { result } = render(deps);
    act(() => result.current.onFileWritten({ tool: "editFile", path: "docs/a.adoc" }));
    expect(deps.editor.reloadTabFromDisk).toHaveBeenCalledWith("a.adoc");
  });

  test("a delete closes tabs under the path instead of reloading them", () => {
    const deps = makeDeps();
    const { result } = render(deps);

    act(() => result.current.onFileWritten({ tool: "deleteFile", path: "docs/a.adoc" }));

    expect(deps.editor.discardTabsUnder).toHaveBeenCalledWith("a.adoc");
    expect(deps.editor.reloadTabFromDisk).not.toHaveBeenCalled();
  });

  test("an unrelated tool still refreshes the tree but touches no tab", () => {
    const deps = makeDeps();
    const { result } = render(deps);

    act(() => result.current.onFileWritten({ tool: "createDirectory", path: "docs/sub" }));

    expect(deps.tree.refresh).toHaveBeenCalled();
    expect(deps.editor.reloadTabFromDisk).not.toHaveBeenCalled();
    expect(deps.editor.discardTabsUnder).not.toHaveBeenCalled();
  });

  test("the OpenAPI bundle is reloaded only when one is open", () => {
    const without = makeDeps("a.adoc", false);
    const { result: r1 } = render(without);
    act(() => r1.current.onFileWritten({ tool: "writeFile", path: "docs/a.adoc" }));
    expect(without.openApiBundle.reload).not.toHaveBeenCalled();

    const withBundle = makeDeps("a.adoc", true);
    const { result: r2 } = render(withBundle);
    act(() => r2.current.onFileWritten({ tool: "writeFile", path: "docs/a.adoc" }));
    expect(withBundle.openApiBundle.reload).toHaveBeenCalled();
  });

  test("a move keeps open tabs pointing at the file's new path", () => {
    const deps = makeDeps();
    const { result } = render(deps);

    act(() =>
      result.current.onFileMoved({
        from: "docs/old.adoc",
        to: "docs/sub/new.adoc",
        updatedFiles: [{ docsRelativePath: "docs/other.adoc", count: 2 }] as never,
      }),
    );

    expect(deps.editor.remapTabsUnder).toHaveBeenCalledWith("old.adoc", "sub/new.adoc");
    expect(deps.session.remapExpandedUnder).toHaveBeenCalledWith("old.adoc", "sub/new.adoc");
    // The destination folder is opened so the moved file is visible.
    expect(deps.session.ensureExpanded).toHaveBeenCalledWith("sub");
    expect(deps.git.scheduleRefresh).toHaveBeenCalled();
  });

  test("a move reports its reference rewrites in docs-relative terms", () => {
    const deps = makeDeps();
    const { result } = render(deps);

    act(() =>
      result.current.onFileMoved({
        from: "docs/old.adoc",
        to: "docs/new.adoc",
        updatedFiles: [{ docsRelativePath: "docs/other.adoc", count: 3 }] as never,
      }),
    );

    expect(renameReports).toEqual([
      { updatedFiles: [{ docsRelativePath: "other.adoc", count: 3 }] },
    ]);
  });
});

describe("useAssistantBridge — text into the editor and chat", () => {
  test("inserting a snippet targets the active tab", () => {
    const { result } = render(makeDeps("a.adoc"));
    act(() => result.current.insertSnippet("== Заголовок"));
    expect(result.current.insertRequest).toMatchObject({
      tabId: "a.adoc",
      text: "== Заголовок",
    });
  });

  test("inserting without an active tab does nothing", () => {
    const { result } = render(makeDeps(null));
    act(() => result.current.insertSnippet("== Заголовок"));
    expect(result.current.insertRequest).toBeNull();
  });

  test("the same snippet twice produces two distinct requests", () => {
    // Otherwise a prop comparing equal would swallow the second insert.
    const { result } = render(makeDeps());
    act(() => result.current.insertSnippet("текст"));
    const first = result.current.insertRequest?.id;
    act(() => result.current.insertSnippet("текст"));
    expect(result.current.insertRequest?.id).not.toBe(first);
  });

  test("adding a selection to chat opens the assistant dock", () => {
    const deps = makeDeps();
    const { result } = render(deps);

    act(() => result.current.addSelectionToChat("выделенное", "a.adoc"));

    expect(deps.layout.setRightTool).toHaveBeenCalledWith("assistant");
    expect(result.current.chatInsertRequest).toMatchObject({
      text: "выделенное",
      filePath: "a.adoc",
    });
  });

  test("the chat request is cleared once the panel consumes it", () => {
    // Cleared here rather than inside the panel, which remounts on a chat
    // switch and would otherwise re-insert the same text.
    const { result } = render(makeDeps());
    act(() => result.current.addSelectionToChat("выделенное", null));
    act(() => result.current.onChatInsertHandled());
    expect(result.current.chatInsertRequest).toBeNull();
  });

  test("sendAssistantPrompt opens the assistant dock and queues a send request", () => {
    const deps = makeDeps();
    const { result } = render(deps);

    act(() => result.current.sendAssistantPrompt("заполни request.adoc", { conversationMode: "agent" }));

    expect(deps.layout.setRightTool).toHaveBeenCalledWith("assistant");
    expect(result.current.assistantSendRequest).toMatchObject({
      text: "заполни request.adoc",
      conversationMode: "agent",
    });
  });

  test("the assistant send request is cleared once the panel consumes it", () => {
    const { result } = render(makeDeps());
    act(() => result.current.sendAssistantPrompt("промпт"));
    act(() => result.current.onAssistantSendHandled());
    expect(result.current.assistantSendRequest).toBeNull();
  });

  test("the same prompt twice produces two distinct send requests", () => {
    const { result } = render(makeDeps());
    act(() => result.current.sendAssistantPrompt("промпт"));
    const first = result.current.assistantSendRequest?.id;
    act(() => result.current.sendAssistantPrompt("промпт"));
    expect(result.current.assistantSendRequest?.id).not.toBe(first);
  });
});
