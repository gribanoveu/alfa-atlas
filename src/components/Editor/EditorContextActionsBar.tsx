import { useState } from "react";
import { Sparkles } from "lucide-react";
import type { EditorContextAction } from "../../lib/editorContextActions";
import { EditorContextActionModal } from "./EditorContextActionModal";
import "./EditorContextActionsBar.css";

type EditorContextActionsBarProps = {
  actions: EditorContextAction[];
  onRunAction: (
    action: EditorContextAction,
    inputValue?: string,
  ) => void;
};

export function EditorContextActionsBar({
  actions,
  onRunAction,
}: EditorContextActionsBarProps) {
  const [modalAction, setModalAction] = useState<EditorContextAction | null>(null);

  if (actions.length === 0) return null;

  const handleClick = (action: EditorContextAction) => {
    if (action.input.kind === "none") {
      onRunAction(action);
      return;
    }
    setModalAction(action);
  };

  const handleModalSubmit = (value: string) => {
    if (!modalAction) return;
    onRunAction(modalAction, value);
    setModalAction(null);
  };

  const modalInput = modalAction?.input;
  const showModal =
    modalAction &&
    modalInput &&
    modalInput.kind !== "none";

  return (
    <>
      <div className="editor-context-actions-bar" aria-label="Предлагаемые действия">
        <Sparkles size={14} strokeWidth={1.75} aria-hidden className="editor-context-actions-icon" />
        <div className="editor-context-actions-list">
          {actions.map((action) => (
            <button
              key={action.id}
              type="button"
              className="editor-context-action-btn"
              onClick={() => handleClick(action)}
            >
              {action.label}
            </button>
          ))}
        </div>
      </div>

      {showModal ? (
        <EditorContextActionModal
          title={modalInput.title}
          placeholder={modalInput.placeholder}
          multiline={modalInput.kind === "text" ? modalInput.multiline !== false : true}
          onCancel={() => setModalAction(null)}
          onSubmit={handleModalSubmit}
        />
      ) : null}
    </>
  );
}
