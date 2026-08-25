import type { LlmMessage } from "./llm";

export type SelectionAiAction =
  | "rewrite"
  | "shorten"
  | "expand"
  | "fix"
  | "custom";

export const SELECTION_AI_MAX_CHARS = 4000;

const SYSTEM_PROMPT = `You are a documentation editor assistant. The user will give you a selected fragment of a document (often AsciiDoc) and ask you to revise it.

Rules:
- Output ONLY the revised fragment — no preamble, no explanation, no quotation marks wrapping the whole reply.
- Do not wrap the reply in markdown code fences.
- Preserve AsciiDoc syntax exactly where it appears (*bold*, _italic_, \`monospace\`, == headings, include::, xref:, image::, links, lists, tables, attributes). Block macros \`include::\`, \`image::\`, and \`xref:\` must always end with \`[]\` (\`include::path.adoc[]\`, \`xref:doc.adoc#anchor[]\`).
- Keep the same language as the source text (Russian stays Russian, English stays English).
- Do not invent facts that are not implied by the source.`;

const ACTION_INSTRUCTIONS: Record<Exclude<SelectionAiAction, "custom">, string> = {
  rewrite: "Rewrite the fragment more clearly and naturally, preserving meaning.",
  shorten: "Shorten the fragment while keeping the key facts and intent.",
  expand: "Expand the fragment with useful detail that fits the existing context.",
  fix: "Fix spelling, punctuation, and style without changing meaning.",
};

/** Best-effort strip of a wrapping markdown code fence — mirrors
 * `services::ai_tools::strip_code_fence` on the Rust side. */
export function stripCodeFence(text: string): string {
  const trimmed = text.trim();
  const lines = trimmed.split("\n");
  if (
    lines.length >= 2 &&
    lines[0].trimStart().startsWith("```") &&
    lines[lines.length - 1].trim() === "```"
  ) {
    return lines.slice(1, -1).join("\n");
  }
  return trimmed;
}

export function buildSelectionAiMessages(
  action: SelectionAiAction,
  selectedText: string,
  customPrompt?: string,
  filePath?: string,
): LlmMessage[] {
  const instruction =
    action === "custom"
      ? (customPrompt?.trim() || "Revise the fragment as requested.")
      : ACTION_INSTRUCTIONS[action];

  const pathLine = filePath ? `File: ${filePath}\n\n` : "";
  const userContent = `${pathLine}${instruction}\n\n---\n${selectedText}\n---`;

  return [
    { role: "system", content: SYSTEM_PROMPT, toolCallId: null },
    { role: "user", content: userContent, toolCallId: null },
  ];
}
