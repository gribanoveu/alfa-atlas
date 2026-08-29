import { describe, expect, test } from "bun:test";
import {
  clampFontSizePx,
  clampGeneralPrefs,
  DEFAULT_GENERAL_PREFS,
  DEFAULT_DIAGRAM_BACKDROP,
  normalizeDiagramBackdrop,
  normalizeDiagramTheme,
  resolveDiagramBackdrop,
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
  test("accepts auto, hex literals and transparent", () => {
    expect(normalizeDiagramBackdrop("auto")).toBe("auto");
    expect(normalizeDiagramBackdrop(" AUTO ")).toBe("auto");
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

describe("resolveDiagramBackdrop", () => {
  test("auto pairs the backdrop with the diagram palette", () => {
    // The whole point of the preset: a dark diagram wants the app's chrome
    // showing through, a light one needs a plate under it. Getting this
    // pair wrong is the readability bug it exists to prevent.
    expect(resolveDiagramBackdrop("auto", "dark")).toBe("transparent");
    expect(resolveDiagramBackdrop("auto", "light")).toBe("#ffffff");
  });

  test("an explicit colour overrides the palette", () => {
    expect(resolveDiagramBackdrop("#1e1f22", "light")).toBe("#1e1f22");
    expect(resolveDiagramBackdrop("transparent", "light")).toBe("transparent");
    expect(resolveDiagramBackdrop("#ffffff", "dark")).toBe("#ffffff");
  });

  test("an invalid colour resolves through the default rather than leaking", () => {
    // Falls back to "auto", which then resolves — never emits the raw
    // string into the CSS custom property.
    expect(resolveDiagramBackdrop("red; } body {", "dark")).toBe("transparent");
    expect(resolveDiagramBackdrop("red; } body {", "light")).toBe("#ffffff");
  });
});

describe("normalizeDiagramTheme", () => {
  test("only light opts out of the dark default", () => {
    expect(normalizeDiagramTheme("light")).toBe("light");
    expect(normalizeDiagramTheme("dark")).toBe("dark");
    expect(normalizeDiagramTheme(undefined)).toBe("dark");
    expect(normalizeDiagramTheme("forest")).toBe("dark");
  });
});
