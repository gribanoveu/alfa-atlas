import { beforeEach, describe, expect, mock, test } from "bun:test";
import { waitFor } from "@testing-library/react";
import type { GeneralPrefs } from "../lib/prefs";
import * as actualPrefs from "../lib/prefs";

let stored: GeneralPrefs;
let writes: GeneralPrefs[] = [];
let writeFails: string | null = null;

mock.module("../lib/prefs", () => ({
  ...actualPrefs,
  getGeneralPrefs: async () => stored,
  setGeneralPrefs: async (next: GeneralPrefs) => {
    if (writeFails) throw writeFails;
    writes.push(next);
    stored = next;
  },
}));

const { chooseDiagramTheme, getDiagramTheme, setDiagramTheme } = await import(
  "../lib/diagramTheme"
);

beforeEach(() => {
  stored = {
    ...actualPrefs.DEFAULT_GENERAL_PREFS,
    diagramTheme: "dark",
    previewFontSizePx: 15,
  };
  writes = [];
  writeFails = null;
  setDiagramTheme("dark");
});

describe("chooseDiagramTheme", () => {
  test("applies the palette immediately, before the write lands", () => {
    chooseDiagramTheme("light");
    // Открытые диаграммы перерисовываются со стора — ждать записи в
    // settings.json они не должны.
    expect(getDiagramTheme()).toBe("light");
  });

  test("persists the choice without disturbing the other preferences", async () => {
    chooseDiagramTheme("light");

    await waitFor(() => expect(writes.length).toBe(1));
    expect(writes[0].diagramTheme).toBe("light");
    // Настройки читаются заново перед записью, поэтому соседние поля
    // переживают переключение — а не затираются копией из чьего-то стейта.
    expect(writes[0].previewFontSizePx).toBe(15);
  });

  test("a failed write leaves the palette applied for the session", async () => {
    writeFails = "settings.json is read-only";
    chooseDiagramTheme("light");

    await waitFor(() => expect(getDiagramTheme()).toBe("light"));
    expect(writes).toEqual([]);
  });
});
