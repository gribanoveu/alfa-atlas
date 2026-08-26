import { useEffect, useRef, useState } from "react";
import { toMessage } from "../../lib/errors";
import type { FileTreeDeleteTarget } from "./FileTree";
import "../Welcome/CloneRepoModal.css";

type RenameModalProps = {
  target: FileTreeDeleteTarget;
  onCancel: () => void;
  onConfirm: (newName: string) => Promise<void>;
};

function basenameOf(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts.length === 0 ? path : parts[parts.length - 1];
}

export function RenameModal({ target, onCancel, onConfirm }: RenameModalProps) {
  const currentName = basenameOf(target.path);
  const [name, setName] = useState(currentName);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const submit = async () => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError("Введите имя.");
      return;
    }
    if (/[/\\]/.test(trimmed) || trimmed === "." || trimmed === "..") {
      setError("Некорректное имя.");
      return;
    }
    if (trimmed === currentName) {
      setError("Имя совпадает с текущим.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onConfirm(trimmed);
    } catch (e) {
      setError(toMessage(e));
      setBusy(false);
    }
  };

  const title = target.isDir ? "Переименовать папку" : "Переименовать файл";

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onClick={onCancel}
    >
      <div
        className="clone-modal"
        role="dialog"
        aria-labelledby="rename-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="rename-title">
          {title}
        </div>

        <div className="clone-modal-message">
          Текущее имя: <b>{currentName}</b>
        </div>

        <label className="clone-modal-field">
          <span className="clone-modal-label">Новое имя</span>
          <input
            ref={inputRef}
            className="clone-modal-input"
            type="text"
            value={name}
            placeholder={currentName}
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
            disabled={busy || !name.trim() || name.trim() === currentName}
          >
            Переименовать
          </button>
        </div>
      </div>
    </div>
  );
}
