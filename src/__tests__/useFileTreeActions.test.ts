import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import * as actualProject from "../lib/project";

let importResults: Array<string | Error | string> = [];
let importCalls: string[] = [];

mock.module("../lib/project", () => ({
  ...actualProject,
  importExternalFile: async (_docsRoot: string, _dest: string, source: string) => {
    importCalls.push(source);
    const next = importResults.shift();
    if (typeof next === "string" && next.startsWith("!")) throw next.slice(1);
    return next ?? "imported.adoc";
  },
}));

const { useFileTreeActions } = await import("../hooks/useFileTreeActions");

let errors: string[] = [];
let successes: string[] = [];

function makeDeps() {
  return {
    project: { docsRoot: "/repo/docs" },
    tree: { refresh: mock(async () => {}) },
    session: { ensureExpanded: mock(() => {}) },
    editor: {
      openFile: mock(async () => {}),
      reloadTabFromDisk: mock(async () => {}),
    },
    git: { scheduleRefresh: mock(() => {}) },
    showSuccess: mock((m: string) => successes.push(m)),
    setError: mock((m: string) => errors.push(m)),
  };
}

function render(overrides: Partial<ReturnType<typeof makeDeps>> = {}) {
  const deps = { ...makeDeps(), ...overrides };
  return { deps, ...renderHook(() => useFileTreeActions(deps as never)) };
}

beforeEach(() => {
  importResults = [];
  importCalls = [];
  errors = [];
  successes = [];
});

describe("useFileTreeActions — rename fallout", () => {
  test("an empty report does nothing at all", async () => {
    const { deps, result } = render();
    await act(async () => {
      await result.current.applyRenameReport({ updatedFiles: [] });
    });
    expect(deps.editor.reloadTabFromDisk).not.toHaveBeenCalled();
    expect(successes).toEqual([]);
  });

  test("every rewritten file is reloaded and the totals are reported", async () => {
    // A rename rewrites include::/image::/xref: in other documents; open
    // tabs must not keep showing the pre-rename text.
    const { deps, result } = render();
    await act(async () => {
      await result.current.applyRenameReport({
        updatedFiles: [
          { docsRelativePath: "a.adoc", count: 2 },
          { docsRelativePath: "b.adoc", count: 3 },
        ],
      });
    });

    expect(deps.editor.reloadTabFromDisk).toHaveBeenCalledTimes(2);
    expect(successes[0]).toBe("Ссылки обновлены — файлов: 2, ссылок: 5");
  });
});

describe("useFileTreeActions — external import", () => {
  test("it opens the last supported file it imported", async () => {
    importResults = ["notes.adoc", "diagram.png"];
    const { deps, result } = render();

    await act(async () => {
      await result.current.importExternal("sub", ["/tmp/notes.adoc", "/tmp/diagram.png"]);
    });

    expect(importCalls).toHaveLength(2);
    expect(deps.editor.openFile).toHaveBeenCalledWith("diagram.png");
    expect(deps.session.ensureExpanded).toHaveBeenCalledWith("sub");
    expect(deps.tree.refresh).toHaveBeenCalled();
    expect(deps.git.scheduleRefresh).toHaveBeenCalled();
  });

  test("an unsupported file is imported but not opened", async () => {
    importResults = ["archive.zip"];
    const { deps, result } = render();

    await act(async () => {
      await result.current.importExternal("sub", ["/tmp/archive.zip"]);
    });

    expect(importCalls).toHaveLength(1);
    expect(deps.editor.openFile).not.toHaveBeenCalled();
  });

  test("one bad file does not abort the rest of the batch", async () => {
    // Dropping ten files and losing nine to one bad one would be worse than
    // a single error message.
    importResults = ["!нет доступа", "second.adoc"];
    const { deps, result } = render();

    await act(async () => {
      await result.current.importExternal("sub", ["/tmp/bad", "/tmp/second.adoc"]);
    });

    expect(importCalls).toHaveLength(2);
    expect(errors).toEqual(["нет доступа"]);
    expect(deps.editor.openFile).toHaveBeenCalledWith("second.adoc");
  });

  test("nothing happens without a docs root", async () => {
    const { deps, result } = render({ project: { docsRoot: null } as never });
    await act(async () => {
      await result.current.importExternal("sub", ["/tmp/a.adoc"]);
    });
    expect(importCalls).toHaveLength(0);
    expect(deps.tree.refresh).not.toHaveBeenCalled();
  });
});

describe("useFileTreeActions — pending dialogs", () => {
  test("each dialog target starts empty and round-trips", () => {
    const { result } = render();
    expect(result.current.deleteTarget).toBeNull();
    expect(result.current.renameTarget).toBeNull();
    expect(result.current.copiedItem).toBeNull();
    expect(result.current.newFileParent).toBeNull();
    expect(result.current.newFolderParent).toBeNull();

    act(() => result.current.setNewFileParent("sub"));
    expect(result.current.newFileParent).toBe("sub");
  });
});
