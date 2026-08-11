import { describe, expect, test } from "bun:test";
import {
  isImageAsset,
  isSupportedFile,
  formatLabelFor,
} from "../lib/supportedFiles";

describe("isImageAsset", () => {
  test("detects common image extensions", () => {
    expect(isImageAsset("images/logo.PNG")).toBe(true);
    expect(isImageAsset("a.svg")).toBe(true);
    expect(isImageAsset("x.webp")).toBe(true);
  });

  test("does not treat docs as images", () => {
    expect(isImageAsset("a.adoc")).toBe(false);
    expect(isSupportedFile("a.png")).toBe(false);
  });

  test("format label for images", () => {
    expect(formatLabelFor("shot.jpg")).toBe("Image");
  });
});
