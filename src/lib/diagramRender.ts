import { renderMermaid } from "../components/AsciiDocPreview/mermaidRenderer";
import { renderPlantuml } from "../components/AsciiDocPreview/plantumlRenderer";
import type { DiagramTheme } from "./prefs";
import type { VisualContent } from "./aiTools";

export type DiagramRenderResult =
  | { kind: "ok"; svg: string }
  | { kind: "error"; message: string };

/** Renders diagram source with whichever engine its format names.
 *
 * A module of its own, rather than the two-line dispatch inlined at the
 * call site, because it is the seam a caller can stub: mocking
 * `mermaidRenderer` directly is process-wide and collides with the tests
 * that exercise the real `renderMermaid` against their own `mermaid` stub.
 * Nothing but `AssistantVisualCard` imports this. */
export function renderDiagram(
  format: VisualContent["format"],
  source: string,
  theme: DiagramTheme,
): Promise<DiagramRenderResult> {
  return format === "plantuml" ? renderPlantuml(source) : renderMermaid(source, theme);
}
