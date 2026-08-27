import { describe, expect, test } from "bun:test";
import {
  clampFontSizePx,
  clampGeneralPrefs,
  DEFAULT_GENERAL_PREFS,
} from "../lib/prefs";

describe("fontSizePrefs", () => {
  test("clampFontSizePx enforces range and half-px steps", () => {
    expect(clampFontSizePx(9)).toBe(10);
    expect(clampFontSizePx(25)).toBe(24);
    expect(clampFontSizePx(13.3)).toBe(13.5);
    expect(clampFontSizePx(14.7)).toBe(14.5);
  });

  test("clampGeneralPrefs applies font defaults for partial prefs", () => {
    const result = clampGeneralPrefs({
      ...DEFAULT_GENERAL_PREFS,
      uiFontSizePx: 30,
      editorFontSizePx: 8.2,
    });
    expect(result.uiFontSizePx).toBe(24);
    expect(result.editorFontSizePx).toBe(10);
    expect(result.sidebarFontSizePx).toBe(DEFAULT_GENERAL_PREFS.sidebarFontSizePx);
    expect(result.previewFontSizePx).toBe(DEFAULT_GENERAL_PREFS.previewFontSizePx);
    expect(result.assistantFontSizePx).toBe(DEFAULT_GENERAL_PREFS.assistantFontSizePx);
  });
});
