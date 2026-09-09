import { useEffect, useId, useRef } from "react";
import "./ConfirmModal.css";

type ConfirmModalProps = {
  title: string;
  message: string;
  /** Defaults to «Продолжить». */
  confirmLabel?: string;
  cancelLabel?: string;
  /** Paints the confirm button as destructive. */
  danger?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
};

/** The app's two-button confirmation dialog — the sibling of `AlertOkModal`,
 * which is the one-button one.
 *
 * Exists because `window.confirm` is not allowed here (AGENTS.md, "UI"): the
 * platform draws it in its own style, ignoring the theme tokens every other
 * surface is built from, and in a webview it blocks the whole UI thread. The
 * shell is generic on purpose — the three callers differ only in their copy,
 * and a fourth bespoke modal file per question is how the existing
 * `*ConfirmModal` set grew to six near-identical ones.
 *
 * Escape cancels and Enter confirms, the two things `window.confirm` gave us
 * for free and that a hand-rolled dialog has to put back. */
export function ConfirmModal({
  title,
  message,
  confirmLabel = "Продолжить",
  cancelLabel = "Отмена",
  danger = false,
  onCancel,
  onConfirm,
}: ConfirmModalProps) {
  const titleId = useId();
  // Through refs so the key handler can be registered once rather than
  // re-bound whenever a caller passes a fresh closure.
  const onCancelRef = useRef(onCancel);
  onCancelRef.current = onCancel;
  const onConfirmRef = useRef(onConfirm);
  onConfirmRef.current = onConfirm;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" && event.key !== "Enter") return;
      event.preventDefault();
      // Capture phase plus `stopPropagation`, because this dialog is opened
      // from inside other modals that close themselves on Escape (see
      // `MemoryLogModal`). Their handlers sit on `document` too, and a plain
      // bubble-phase listener would let one Escape dismiss this question and
      // the modal behind it in the same keystroke.
      event.stopPropagation();
      if (event.key === "Escape") onCancelRef.current();
      else onConfirmRef.current();
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, []);

  return (
    <div
      className="confirm-modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
      // Same reason as the capture-phase key handler: a host modal that
      // closes on a backdrop click must not also receive this one.
      onClick={(event) => event.stopPropagation()}
    >
      <div
        className="confirm-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="confirm-modal-title" id={titleId}>
          {title}
        </div>
        <div className="confirm-modal-message">{message}</div>
        <div className="confirm-modal-actions">
          <button type="button" className="confirm-modal-btn" onClick={onCancel}>
            {cancelLabel}
          </button>
          <button
            type="button"
            className={`confirm-modal-btn ${danger ? "danger" : "primary"}`}
            onClick={onConfirm}
            autoFocus
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
