import { useEffect, useRef, useState } from "react";
import "../Welcome/CloneRepoModal.css";

type NewFolderModalProps = {
  parentPath: string;
  onCancel: () => void;
  onConfirm: (folderName: string) => Promise<void>;
};

export function NewFolderModal({
  parentPath,
  onCancel,
  onConfirm,
}: NewFolderModalProps) {
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const submit = async () => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError("Введите имя папки.");
      return;
    }
    if (/[/\\]/.test(trimmed) || trimmed === "." || trimmed === "..") {
      setError("Некорректное имя папки.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onConfirm(trimmed);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  const location =
    !parentPath || parentPath === "." ? "(корень документации)" : parentPath;

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onClick={onCancel}
    >
      <div
        className="clone-modal"
        role="dialog"
        aria-labelledby="new-folder-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="new-folder-title">
          Новая папка
        </div>

        <div className="clone-modal-message">
          Родитель: <b>{location}</b>
        </div>

        <label className="clone-modal-field">
          <span className="clone-modal-label">Имя</span>
          <input
            ref={inputRef}
            className="clone-modal-input"
            type="text"
            value={name}
            placeholder="methods"
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void submit();
            }}
          />
        </label>

        {error ? <div className="clone-modal-message">{error}</div> : null}

        <div className="clone-modal-actions">
          <button type="button" className="clone-modal-btn" onClick={onCancel}>
            Отмена
          </button>
          <button
            type="button"
            className="clone-modal-btn primary"
            onClick={() => void submit()}
            disabled={busy || !name.trim()}
          >
            Создать
          </button>
        </div>
      </div>
    </div>
  );
}
