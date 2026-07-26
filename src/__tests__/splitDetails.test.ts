import { describe, expect, test } from "bun:test";
import {
  expandSplitDetails,
  isSplitDetailsClose,
  isSplitDetailsOpen,
  parseDetailsOpen,
} from "../components/AsciiDocPreview/splitDetails";

describe("splitDetails", () => {
  test("isSplitDetailsOpen detects unclosed details pass", () => {
    expect(isSplitDetailsOpen("<details>\n<summary>Click me</summary>")).toBe(
      true,
    );
    expect(
      isSplitDetailsOpen("<details open class=\"x\"><summary>Title</summary>"),
    ).toBe(true);
    expect(
      isSplitDetailsOpen(
        "<details><summary>X</summary><p>Hidden</p></details>",
      ),
    ).toBe(false);
  });

  test("isSplitDetailsClose detects closing pass", () => {
    expect(isSplitDetailsClose("</details>")).toBe(true);
    expect(isSplitDetailsClose("  </details>  ")).toBe(true);
    expect(isSplitDetailsClose("<details></details>")).toBe(false);
  });

  test("parseDetailsOpen extracts summary html", () => {
    expect(
      parseDetailsOpen("<details>\n<summary>Click *me*</summary>"),
    ).toEqual({
      detailsAttrs: "",
      summaryHtml: "Click *me*",
      leadingHtml: "<summary>Click *me*</summary>",
    });
  });

  test("expandSplitDetails merges pass / blocks / pass sequence", () => {
    const passOpen = {
      getContext: () => "pass",
      getSource: () => "<details>\n<summary>Title</summary>",
    };
    const paragraph = {
      getContext: () => "paragraph",
      getSource: () => undefined,
    };
    const passClose = {
      getContext: () => "pass",
      getSource: () => "</details>",
    };

    const expanded = expandSplitDetails([
      passOpen,
      paragraph,
      passClose,
    ] as never);

    expect(expanded).toHaveLength(1);
    expect(expanded[0]?.kind).toBe("split-details");
    if (expanded[0]?.kind === "split-details") {
      expect(expanded[0].openSource).toContain("<details>");
      expect(expanded[0].innerBlocks).toHaveLength(1);
    }
  });

  test("expandSplitDetails leaves unmatched open pass unchanged", () => {
    const passOpen = {
      getContext: () => "pass",
      getSource: () => "<details><summary>Only open</summary>",
    };
    const paragraph = {
      getContext: () => "paragraph",
    };

    const expanded = expandSplitDetails([passOpen, paragraph] as never);

    expect(expanded).toHaveLength(2);
    expect(expanded[0]?.kind).toBe("block");
    expect(expanded[1]?.kind).toBe("block");
  });
});
