import { useMemo, useState } from "react";
import type { ApprovalSubmission, PendingReview } from "../../hooks/useLlmChat";
import type { PendingToolCall } from "../../lib/llm";
import { WriteFileDiffReview } from "./WriteFileDiffReview";
import "../Welcome/CloneRepoModal.css";
import "./AssistantPanel.css";

type ToolApprovalModalProps = {
  review: PendingReview;
  docsRoot: string | null;
  onSubmit: (submission: ApprovalSubmission) => void;
};

const TOOL_LABELS: Record<string, { title: string; trustLabel: string }> = {
  writeFile: {
    title: "Изменение файла",
    trustLabel: "Не спрашивать об изменениях файлов до конца этого диалога",
  },
  requestFullRepoAccess: {
    title: "Запрос доступа к репозиторию",
    trustLabel: "Не спрашивать о запросах доступа до конца этого диалога",
  },
};

function parseArgs(json: string): Record<string, unknown> {
  try {
    const parsed: unknown = JSON.parse(json);
    return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

/** Reviews every call in one paused round together — a single batch, not
 * one prompt per call — with per-card Approve/Deny, bulk actions, and a
 * per-tool "don't ask again this conversation" checkbox. See
 * `useLlmChat`'s `awaitApproval`/`submitApprovalDecisions` for how this
 * plugs into the resume loop. */
export function ToolApprovalModal({ review, docsRoot, onSubmit }: ToolApprovalModalProps) {
  const [decisions, setDecisions] = useState<Record<string, boolean>>({});
  const [trustNames, setTrustNames] = useState<Set<string>>(new Set());

  const toolNames = useMemo(() => Array.from(new Set(review.calls.map((c) => c.name))), [review.calls]);
  const allDecided = review.calls.every((c) => c.id in decisions);

  const setAll = (approved: boolean) => {
    setDecisions(Object.fromEntries(review.calls.map((c) => [c.id, approved])));
  };

  const toggleTrust = (name: string, checked: boolean) => {
    setTrustNames((prev) => {
      const next = new Set(prev);
      if (checked) next.add(name);
      else next.delete(name);
      return next;
    });
  };

  const handleSubmit = () => {
    onSubmit({
      decisions: review.calls.map((c) => ({ id: c.id, approved: decisions[c.id] ?? false })),
      trustToolNames: Array.from(trustNames),
    });
  };

  return (
    <div className="clone-modal-backdrop" role="presentation">
      <div
        className="clone-modal tool-approval-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="tool-approval-title"
      >
        <div className="clone-modal-title" id="tool-approval-title">
          Ассистент запрашивает подтверждение
        </div>
        <div className="clone-modal-message">
          {review.calls.length === 1
            ? "Проверьте действие перед тем, как разрешить его."
            : `Проверьте ${review.calls.length} действия перед тем, как разрешить их.`}
        </div>

        <div className="tool-approval-list">
          {review.calls.map((call) => (
            <ToolApprovalCard
              key={call.id}
              call={call}
              docsRoot={docsRoot}
              decision={decisions[call.id]}
              onDecide={(approved) => setDecisions((prev) => ({ ...prev, [call.id]: approved }))}
            />
          ))}
        </div>

        {toolNames.map((name) => (
          <label key={name} className="tool-approval-trust">
            <input
              type="checkbox"
              checked={trustNames.has(name)}
              onChange={(e) => toggleTrust(name, e.target.checked)}
            />
            {TOOL_LABELS[name]?.trustLabel ?? `Не спрашивать про «${name}» до конца этого диалога`}
          </label>
        ))}

        <div className="clone-modal-actions">
          <button type="button" className="clone-modal-btn" onClick={() => setAll(false)}>
            Отклонить всё
          </button>
          <button type="button" className="clone-modal-btn" onClick={() => setAll(true)}>
            Одобрить всё
          </button>
          <button type="button" className="clone-modal-btn primary" disabled={!allDecided} onClick={handleSubmit}>
            Подтвердить
          </button>
        </div>
      </div>
    </div>
  );
}

function ToolApprovalCard({
  call,
  docsRoot,
  decision,
  onDecide,
}: {
  call: PendingToolCall;
  docsRoot: string | null;
  decision: boolean | undefined;
  onDecide: (approved: boolean) => void;
}) {
  const args = useMemo(() => parseArgs(call.arguments), [call.arguments]);
  const label = TOOL_LABELS[call.name]?.title ?? call.name;

  return (
    <div className="tool-approval-card">
      <div className="tool-approval-card-head">
        <span className="tool-approval-card-title">{label}</span>
        <div className="tool-approval-card-buttons">
          <button
            type="button"
            className={`tool-approval-decision-btn approve ${decision === true ? "active" : ""}`}
            onClick={() => onDecide(true)}
          >
            Одобрить
          </button>
          <button
            type="button"
            className={`tool-approval-decision-btn deny ${decision === false ? "active" : ""}`}
            onClick={() => onDecide(false)}
          >
            Отклонить
          </button>
        </div>
      </div>

      {call.name === "writeFile" && typeof args.path === "string" && typeof args.content === "string" ? (
        <>
          <div className="tool-approval-card-path">{args.path}</div>
          {docsRoot ? <WriteFileDiffReview docsRoot={docsRoot} path={args.path} content={args.content} /> : null}
        </>
      ) : call.name === "requestFullRepoAccess" && typeof args.reason === "string" ? (
        <div className="tool-approval-card-reason">{args.reason}</div>
      ) : (
        <pre className="tool-approval-card-raw">{call.arguments}</pre>
      )}
    </div>
  );
}
