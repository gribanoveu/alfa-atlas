import type { ToolCallBlock } from "./chatBlocks";
import type { VisualContent } from "./aiTools";

/** Visualizations the assistant draws with the `visualize` tool: the tab-id
 *  encoding (mirroring `artifactTabId` in `./artifactTabs.ts` and
 *  `planTabId` in `useEditorTabs`) plus the reader that recovers one from a
 *  chat message.
 *
 *  Unlike plans and artifacts, a visualization has **no store**. It is not
 *  a document the user works on, it is something the assistant said — so it
 *  lives where everything else the assistant said lives, on the chat's own
 *  tool-call block, which `chatHistory.saveChat` already persists. That is
 *  why `openVisualTab` carries the whole payload rather than an id the tab
 *  could look up: `visualFromBlock` is the only reader, and the chat card
 *  is the only thing that has a block to read.
 *
 *  Opening happens through the `atlas-open-visual` window event rather than
 *  a prop chain — the same cross-component escape hatch `atlas-open-plan`
 *  and `atlas-open-artifact` already use, since the chat panel and the
 *  editor sit in different subtrees. */

export type { VisualContent } from "./aiTools";

export type Visual = {
  id: string;
  title: string;
  caption?: string;
  content: VisualContent;
};

const TAB_ID_PREFIX = "visual:";

export function visualTabId(visualId: string): string {
  return `${TAB_ID_PREFIX}${visualId}`;
}

export function visualIdFromTabId(tabId: string): string | null {
  if (!tabId.startsWith(TAB_ID_PREFIX)) return null;
  const id = tabId.slice(TAB_ID_PREFIX.length);
  return id.length > 0 ? id : null;
}

/** Human label per format, for the tab eyebrow and the save dialog. */
export const DIAGRAM_FORMAT_LABELS: Record<VisualContent["format"], string> = {
  mermaid: "Mermaid",
  plantuml: "PlantUML",
};

/** File extension a diagram of this format saves as — both are already in
 *  `supportedFiles.ts`, so a saved file reopens with the same renderer. */
export const DIAGRAM_FORMAT_EXTENSIONS: Record<VisualContent["format"], string> = {
  mermaid: "mmd",
  plantuml: "puml",
};

/** Dispatches the request to open `visual` in an editor tab. `App` listens
 *  for this. */
export function openVisualTab(visual: Visual): void {
  window.dispatchEvent(new CustomEvent("atlas-open-visual", { detail: visual }));
}

function isDiagramFormat(value: unknown): value is VisualContent["format"] {
  return value === "mermaid" || value === "plantuml";
}

/** Recovers a `Visual` from a settled `visualize` tool-call block: the id
 *  from the result (the backend mints it), everything else from the call's
 *  own arguments — the same split `AssistantPlanCard` uses to read a plan's
 *  name off a settled `createPlan`.
 *
 *  Returns `null` for a block that is still running, errored, or whose
 *  arguments do not parse into something renderable. A caller showing a
 *  card should treat `null` as "nothing to open yet", not as an error —
 *  `AssistantVisualCard` renders its own running/error states from the
 *  block's `status` instead. */
export function visualFromBlock(block: ToolCallBlock): Visual | null {
  if (block.status !== "done" || !block.result) return null;
  if (block.result.tool !== "visualShown") return null;
  const { visualId, title } = block.result.result;
  if (!visualId) return null;

  let args: Record<string, unknown>;
  try {
    args = JSON.parse(block.argumentsJson) as Record<string, unknown>;
  } catch {
    // The result alone cannot rebuild the diagram — it deliberately does
    // not carry the source — so a block whose arguments are unreadable has
    // nothing left to render.
    return null;
  }

  if (args.kind !== "diagram") return null;
  if (!isDiagramFormat(args.format)) return null;
  if (typeof args.source !== "string" || args.source.trim() === "") return null;

  const caption = typeof args.caption === "string" && args.caption.trim() !== "" ? args.caption : undefined;

  return {
    id: visualId,
    // Prefer the result's title: the backend trimmed it, and it is what the
    // model was told the tab would be called.
    title: title || (typeof args.title === "string" ? args.title : "Схема"),
    caption,
    content: { kind: "diagram", format: args.format, source: args.source },
  };
}
