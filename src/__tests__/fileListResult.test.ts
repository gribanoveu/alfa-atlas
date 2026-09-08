import { describe, expect, test } from "bun:test";
import { normalizeFileListResult } from "../lib/aiTools";
import { describeToolResult } from "../lib/assistantConfig";

/** `ToolResult::FileList` went from a bare array to `{ entries, truncated }`
 * (`domain::ai_tools`); the renderers call `.filter`/`.map` on it, so a
 * shape they don't expect isn't a wrong label — it throws and takes the
 * whole chat panel down. Both spellings reach them: the new one live, the
 * old one out of persisted chat history. */
describe("fileList result shape", () => {
  test("current wire shape", () => {
    expect(
      normalizeFileListResult({ entries: [{ path: "a.adoc", isDir: false }], truncated: true }),
    ).toEqual({ entries: [{ path: "a.adoc", isDir: false }], truncated: true });
  });

  test("array from a chat saved before `truncated` existed", () => {
    expect(normalizeFileListResult([{ path: "docs", isDir: true }])).toEqual({
      entries: [{ path: "docs", isDir: true }],
      truncated: false,
    });
  });

  test("summary counts entries in either shape", () => {
    const summary = (result: Parameters<typeof normalizeFileListResult>[0]) =>
      describeToolResult({
        name: "listFiles",
        status: "done",
        result: { tool: "fileList", result },
        errorMessage: null,
      });

    const entries = [
      { path: "a.adoc", isDir: false },
      { path: "docs", isDir: true },
    ];
    expect(summary({ entries, truncated: false })).toBe("файлов: 1, папок: 1");
    expect(summary({ entries, truncated: true })).toBe("файлов: 1, папок: 1, обрезано");
    expect(summary(entries)).toBe("файлов: 1, папок: 1");
    expect(summary({ entries: [], truncated: false })).toBe("Пусто");
  });
});
