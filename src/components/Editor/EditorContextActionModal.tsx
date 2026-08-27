import "../Welcome/CloneRepoModal.css";
import "./EditorContextActionsBar.css";

type EditorContextActionModalProps = {
  title: string;
  placeholder: string;
  multiline?: boolean;
  submitLabel?: string;
  onCancel: () => void;
  onSubmit: (value: string) => void;
};

export function EditorContextActionModal({
  title,
  placeholder,
  multiline = true,
  submitLabel = "Отправить",
  onCancel,
  onSubmit,
}: EditorContextActionModalProps) {
  const handleSubmit = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const field = form.elements.namedItem("input") as HTMLTextAreaElement | HTMLInputElement;
    const value = field.value.trim();
    if (!value) return;
    onSubmit(value);
  };

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onClick={onCancel}
    >
      <div
        className="clone-modal editor-context-action-modal"
        role="dialog"
        aria-labelledby="editor-context-action-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="editor-context-action-title">
          {title}
        </div>

        <form className="editor-context-action-form" onSubmit={handleSubmit}>
          {multiline ? (
            <textarea
              id="editor-context-action-input"
              name="input"
              className="editor-context-action-input"
              placeholder={placeholder}
              rows={8}
              autoFocus
            />
          ) : (
            <input
              id="editor-context-action-input"
              name="input"
              type="text"
              className="editor-context-action-input editor-context-action-input-single"
              placeholder={placeholder}
              autoFocus
            />
          )}

          <div className="clone-modal-actions">
            <button type="button" className="clone-modal-btn" onClick={onCancel}>
              Отмена
            </button>
            <button type="submit" className="clone-modal-btn primary">
              {submitLabel}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
