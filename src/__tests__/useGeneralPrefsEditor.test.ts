import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { GeneralPrefs } from "../lib/prefs";
import * as actualPrefs from "../lib/prefs";

const DEFAULTS = {
  uiFontSizePx: 13,
  sidebarFontSizePx: 13,
  editorFontSizePx: 14,
  previewFontSizePx: 15,
  assistantFontSizePx: 13,
  autosaveEnabled: true,
  autosaveDelayMs: 1000,
} as unknown as GeneralPrefs;

let stored: GeneralPrefs;
let writeFails: string | null = null;
let writes: GeneralPrefs[] = [];

mock.module("../lib/prefs", () => ({
  ...actualPrefs,
  DEFAULT_GENERAL_PREFS: DEFAULTS,
  getGeneralPrefs: async () => stored,
  getSettingsPaths: async () => ({ userSettingsDir: "/home/u/.atlas" }),
  setGeneralPrefs: async (next: GeneralPrefs) => {
    if (writeFails) throw writeFails;
    writes.push(next);
    stored = next;
  },
}));
mock.module("@tauri-apps/plugin-opener", () => ({
  openPath: async () => {},
  openUrl: async () => {},
}));

const { useGeneralPrefsEditor } = await import("../hooks/useGeneralPrefsEditor");

beforeEach(() => {
  stored = { ...DEFAULTS, uiFontSizePx: 16 } as GeneralPrefs;
  writeFails = null;
  writes = [];
});

describe("useGeneralPrefsEditor", () => {
  test("prefs stay null until the real values load", async () => {
    const { result } = renderHook(() => useGeneralPrefsEditor());
    // Unlike `useGeneralPrefs`, this must not show a default the user never
    // chose — the next toggle would persist it.
    expect(result.current.prefs).toBeNull();

    await waitFor(() => expect(result.current.prefs).not.toBeNull());
    expect(result.current.prefs?.uiFontSizePx).toBe(16);
    expect(result.current.paths?.userSettingsDir).toBe("/home/u/.atlas");
  });

  test("a patch writes through and notifies the app", async () => {
    const seen: GeneralPrefs[] = [];
    const { result } = renderHook(() => useGeneralPrefsEditor((p) => seen.push(p)));
    await waitFor(() => expect(result.current.prefs).not.toBeNull());

    await act(async () => {
      result.current.patchPrefs({ uiFontSizePx: 20 });
    });

    expect(writes.at(-1)?.uiFontSizePx).toBe(20);
    expect(seen.at(-1)?.uiFontSizePx).toBe(20);
    expect(result.current.prefs?.uiFontSizePx).toBe(20);
  });

  test("a failed write rolls back to what the backend actually holds", async () => {
    const { result } = renderHook(() => useGeneralPrefsEditor());
    await waitFor(() => expect(result.current.prefs).not.toBeNull());
    writeFails = "disk full";

    await act(async () => {
      result.current.patchPrefs({ uiFontSizePx: 99 });
    });

    // The control must not stay on a value that was never saved.
    expect(result.current.prefs?.uiFontSizePx).toBe(16);
    expect(result.current.error).toBe("disk full");
    expect(result.current.busy).toBe(false);
  });

  test("staging previews without writing; persisting saves", async () => {
    const seen: GeneralPrefs[] = [];
    const { result } = renderHook(() => useGeneralPrefsEditor((p) => seen.push(p)));
    await waitFor(() => expect(result.current.prefs).not.toBeNull());

    // Dragging a slider: the app re-renders at the new size, nothing is
    // written yet.
    act(() => result.current.stagePref({ editorFontSizePx: 22 }));
    expect(result.current.prefs?.editorFontSizePx).toBe(22);
    expect(seen.at(-1)?.editorFontSizePx).toBe(22);
    expect(writes).toHaveLength(0);

    await act(async () => {
      result.current.persistPref({ editorFontSizePx: 22 });
    });
    expect(writes.at(-1)?.editorFontSizePx).toBe(22);
  });

  test("resetting fonts restores every font default in one write", async () => {
    stored = {
      ...DEFAULTS,
      uiFontSizePx: 20,
      sidebarFontSizePx: 20,
      editorFontSizePx: 20,
      previewFontSizePx: 20,
      assistantFontSizePx: 20,
      autosaveDelayMs: 4000,
    } as GeneralPrefs;
    const { result } = renderHook(() => useGeneralPrefsEditor());
    await waitFor(() => expect(result.current.prefs).not.toBeNull());

    await act(async () => {
      result.current.resetFontPrefs();
    });

    expect(writes).toHaveLength(1);
    const written = writes[0]!;
    expect(written.uiFontSizePx).toBe(DEFAULTS.uiFontSizePx);
    expect(written.previewFontSizePx).toBe(DEFAULTS.previewFontSizePx);
    expect(written.assistantFontSizePx).toBe(DEFAULTS.assistantFontSizePx);
    // Non-font settings are untouched by a font reset.
    expect(written.autosaveDelayMs).toBe(4000);
  });
});
