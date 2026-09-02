import { ChevronDown, ChevronRight, Shield } from "lucide-react";
import { useState } from "react";
import { AUTO_APPROVABLE_TOOL_LABELS, describeToolActivity, isAutoApprovable } from "../../lib/assistantConfig";
import type { ToolCallBlock } from "../../lib/chatBlocks";
import { ApprovalCountdown, editCountBadge, isExpandableToolCall, ToolApprovalPreview } from "./ToolApprovalPreview";

type AssistantToolApprovalGroupProps = {
  blocks: ToolCallBlock[];
  /** The open project's docs root — forwarded to each item's
   * `ToolApprovalPreview` for its diff fetch. */
  docsRoot: string;
  repoRoot: string;
  /** Called once per block on submit — see `useLlmChat`'s `decideToolCall`. */
  onDecide: (id: string, approved: boolean, trust: boolean) => void;
};

/** One combined card for every call in a paused round, replacing what used
 * to be one `ToolApprovalCard` per call. `useLlmChat`'s `collectDecisions`
 * already treats a round as all-or-nothing-until-everyone-answers — nothing
 * executes until every call has a decision — so this card just makes that
 * existing behavior visible instead of presenting each call as its own
 * independent, immediately-actionable prompt.
 *
 * Each row has an include checkbox (checked by default); "Одобрить" approves
 * every still-checked row and denies every unchecked one in a single click.
 * "Разрешать всегда" is offered per distinct tool *name* present in the
 * batch, not as one blanket trust-everything toggle — ticking one force-
 * approves every call of that name in this batch (regardless of its own
 * checkbox) and persists the grant for future calls, mirroring the old
 * single-card semantics at the granularity that already exists
 * (`ai_auto_approved_tools` is keyed by tool name, never by path or batch).
 * Only tools `isAutoApprovable` accepts get that row: for a consent tool
 * (widening access, switching mode) the pause *is* the feature, so the
 * whole trust block disappears when a batch holds nothing else.
 *
 * Always used even for a round with exactly one confirmable call — no
 * separate one-item code path, see `groupBlocksForRender`. */
export function AssistantToolApprovalGroup({ blocks, docsRoot, repoRoot, onDecide }: AssistantToolApprovalGroupProps) {
  const [included, setIncluded] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(blocks.map((b) => [b.id, true])),
  );
  const [trustedNames, setTrustedNames] = useState<Record<string, boolean>>({});
  const [decided, setDecided] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  const distinctNames = [...new Set(blocks.map((b) => b.name))];
  const trustableNames = distinctNames.filter(isAutoApprovable);
  const multiple = blocks.length > 1;
  // The round can also be settled from outside this card — the approval
  // countdown expiring, or Stop denying everything. Without this the
  // buttons stayed live on an already-answered round and a click did
  // nothing at all, which reads as the app ignoring the user.
  const settledElsewhere = blocks.some((b) => b.status !== "pendingApproval");
  const locked = decided || settledElsewhere;

  const handleDecideAll = (mode: "approveSelected" | "denyAll") => {
    if (locked) return;
    setDecided(true);
    for (const block of blocks) {
      if (mode === "denyAll") {
        onDecide(block.id, false, false);
        continue;
      }
      const trust = Boolean(trustedNames[block.name]);
      const approved = trust || (included[block.id] ?? true);
      onDecide(block.id, approved, trust);
    }
  };

  return (
    <div className="assistant-tool-approval-group">
      {multiple ? (
        <div className="assistant-tool-approval-group-header">Запрошено действий: {blocks.length}</div>
      ) : null}

      <ul className="assistant-tool-approval-group-list">
        {blocks.map((block) => {
          const title = describeToolActivity(block.name, block.argumentsJson);
          const expandable = isExpandableToolCall(block);
          const badge = editCountBadge(block);
          const isExpanded = expanded[block.id] ?? false;
          return (
            <li key={block.id} className="assistant-tool-approval-group-item">
              <div className="assistant-tool-approval-group-item-row">
                {/* Only a batch has anything to pick from. On a single call
                    the checkbox offers a second way to say what "Отклонить"
                    already says, and on a consent request (widening access,
                    switching mode) — which is almost always alone in its
                    round — it is the only thing on the card that does not
                    look like a decision. */}
                {multiple ? (
                  <input
                    type="checkbox"
                    className="assistant-tool-approval-group-item-checkbox"
                    checked={included[block.id] ?? true}
                    disabled={locked}
                    aria-label={title}
                    onChange={(e) => setIncluded((prev) => ({ ...prev, [block.id]: e.target.checked }))}
                  />
                ) : null}
                {expandable ? (
                  <button
                    type="button"
                    className="assistant-tool-approval-group-item-title assistant-tool-approval-group-item-title-toggle"
                    aria-expanded={isExpanded}
                    onClick={() => setExpanded((prev) => ({ ...prev, [block.id]: !isExpanded }))}
                  >
                    {isExpanded ? (
                      <ChevronDown className="assistant-tool-call-chevron" size={12} aria-hidden />
                    ) : (
                      <ChevronRight className="assistant-tool-call-chevron" size={12} aria-hidden />
                    )}
                    <span>{title}</span>
                    {badge ? <span className="assistant-tool-approval-edit-count">{badge}</span> : null}
                  </button>
                ) : (
                  <span className="assistant-tool-approval-group-item-title">{title}</span>
                )}
              </div>
              <div className="assistant-tool-approval-group-item-preview">
                <ToolApprovalPreview block={block} docsRoot={docsRoot} repoRoot={repoRoot} expanded={isExpanded} />
              </div>
            </li>
          );
        })}
      </ul>

      {trustableNames.length > 0 ? (
        <div className="assistant-tool-approval-group-trust">
          <div className="assistant-tool-approval-group-trust-heading">
            <Shield size={13} aria-hidden />
            <span>Больше не спрашивать в этом проекте для:</span>
          </div>
          {trustableNames.map((name) => (
            <label key={name} className="assistant-tool-approval-group-trust-item">
              <input
                type="checkbox"
                checked={Boolean(trustedNames[name])}
                disabled={locked}
                onChange={(e) => setTrustedNames((prev) => ({ ...prev, [name]: e.target.checked }))}
              />
              <span>{AUTO_APPROVABLE_TOOL_LABELS[name] ?? name}</span>
            </label>
          ))}
        </div>
      ) : null}

      <div className="assistant-tool-approval-actions">
        <div className="assistant-tool-approval-buttons">
          <button
            type="button"
            className="assistant-btn"
            disabled={locked}
            onClick={() => handleDecideAll("denyAll")}
          >
            {multiple ? "Отклонить всё" : "Отклонить"}
          </button>
          <button
            type="button"
            className="assistant-btn primary"
            disabled={locked}
            onClick={() => handleDecideAll("approveSelected")}
          >
            {multiple ? "Одобрить выбранные" : "Одобрить"}
          </button>
        </div>
      </div>

      <ApprovalCountdown deadlineAt={blocks[0]?.deadlineAt} />
    </div>
  );
}
