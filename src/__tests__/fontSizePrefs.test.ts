import { describe, expect, test } from "bun:test";
import {
  clampFontSizePx,
  clampGeneralPrefs,
  DEFAULT_GENERAL_PREFS,
  DEFAULT_DIAGRAM_BACKDROP,
  normalizeDiagramBackdrop,
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

describe("normalizeDiagramBackdrop", () => {
  test("accepts hex literals and transparent", () => {
    expect(normalizeDiagramBackdrop("#FFF")).toBe("#fff");
    expect(normalizeDiagramBackdrop("#1e1f22")).toBe("#1e1f22");
    expect(normalizeDiagramBackdrop("#1E1F22AA")).toBe("#1e1f22aa");
    expect(normalizeDiagramBackdrop(" transparent ")).toBe("transparent");
  });

  test("falls back for anything that could escape a CSS declaration", () => {
    // The value goes into a CSS custom property, which React does not
    // escape — a string that can close the declaration must not survive.
    for (const hostile of [
      "red; background-image: url(http://evil/x)",
      "#fff; } body { display: none",
      "url(http://evil/x)",
      "white",
      "#12345",
      "#gggggg",
      "",
    ]) {
      expect(normalizeDiagramBackdrop(hostile)).toBe(DEFAULT_DIAGRAM_BACKDROP);
    }
  });

  test("tolerates a value missing from an older settings.json", () => {
    expect(normalizeDiagramBackdrop(undefined)).toBe(DEFAULT_DIAGRAM_BACKDROP);
    expect(normalizeDiagramBackdrop(null)).toBe(DEFAULT_DIAGRAM_BACKDROP);
  });
});
