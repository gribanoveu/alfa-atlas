import { AlertCircle, Check, Loader2 } from "lucide-react";
import { describeToolActivity, describeToolResult } from "../../lib/assistantConfig";
import type { ToolCallBlock } from "../../lib/chatBlocks";

type AssistantToolCallBlockProps = {
  block: ToolCallBlock;
};

/** One permanent, chronological entry for a single tool invocation inside
 * an assistant message's transcript — a status icon, the "what is/was being
 * done" line (`describeToolActivity`, reused as-is regardless of status),
 * and once settled, a dimmed one-line result summary (`describeToolResult`).
 * Never disappears once appended, unlike the old transient `toolActivity`
 * list it replaces — see `useLlmChat`'s `MessageBlock` model. */
export function AssistantToolCallBlock({ block }: AssistantToolCallBlockProps) {
  return (
    <div className={`assistant-tool-call assistant-tool-call-${block.status}`}>
      <div className="assistant-tool-call-header">
        {block.status === "running" ? (
          <Loader2 className="assistant-tool-call-icon assistant-chat-tool-spinner" size={13} aria-hidden />
        ) : block.status === "done" ? (
          <Check className="assistant-tool-call-icon" size={13} aria-hidden />
        ) : (
          <AlertCircle className="assistant-tool-call-icon" size={13} aria-hidden />
        )}
        <span className="assistant-tool-call-label">{describeToolActivity(block.name, block.argumentsJson)}</span>
      </div>
      {block.status !== "running" ? (
        <div className="assistant-tool-call-summary">{describeToolResult(block)}</div>
      ) : null}
    </div>
  );
}
