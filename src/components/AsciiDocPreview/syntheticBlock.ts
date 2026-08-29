import type { AbstractBlock } from "./types";

/**
 * A fake asciidoctor "listing" block wrapping raw diagram source.
 *
 * `AscPlantuml` and `AscMermaid` read their source through the asciidoctor
 * AST (`block.getSource()`) and their title through the first positional
 * block attribute (`getAttribute("1")`) — which is exactly the whole
 * interface they need. Handing them one of these lets any source string be
 * rendered by the same viewer, with the same zoom/pan/fit toolbar, whether
 * it came from a `[mermaid] ---- … ----` block inside an `.adoc`, from a
 * standalone `.mmd`/`.puml` file (`AsciiDocPreview`), or from the
 * assistant's `visualize` tool (`VisualView`) — none of which has a real
 * AST behind it.
 */
export function makeDiagramBlock(source: string, name: string | null): AbstractBlock {
  return {
    getSource: () => source,
    getAttribute: (key: string) => (key === "1" ? name : null),
  } as unknown as AbstractBlock;
}
