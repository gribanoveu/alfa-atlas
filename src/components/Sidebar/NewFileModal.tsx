import { useEffect, useRef, useState } from "react";
import {
  DEFAULT_NEW_FILE_EXTENSION,
  NEW_FILE_EXTENSION_OPTIONS,
} from "../../lib/supportedFiles";
import "../Welcome/CloneRepoModal.css";

type NewFileModalProps = {
  parentPath: string;
  onCancel: () => void;
  onConfirm: (fileName: string) => Promise<void>;
};

function stripMatchingExtension(name: string, ext: string): string {
  const lower = name.toLowerCase();
  if (lower.endsWith(ext.toLowerCase())) {
    return name.slice(0, name.length - ext.length);
  }
  return name;
}

export function NewFileModal({
  parentPath,
  onCancel,
  onConfirm,
}: NewFileModalProps) {
  const [name, setName] = useState("");
  const [ext, setExt] = useState<string>(DEFAULT_NEW_FILE_EXTENSION);
  const [extOpen, setExtOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const extRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const selected =
    NEW_FILE_EXTENSION_OPTIONS.find((o) => o.ext === ext) ??
    NEW_FILE_EXTENSION_OPTIONS[0];

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    if (!extOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!extRef.current?.contains(event.target as Node)) {
        setExtOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setExtOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [extOpen]);

  const submit = async () => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError("Введите имя файла.");
      return;
    }
    if (/[/\\]/.test(trimmed)) {
      setError("Имя файла не должно содержать путь.");
      return;
    }
    const base = stripMatchingExtension(trimmed, ext);
    if (!base.trim()) {
      setError("Введите имя файла.");
      return;
    }
    const fileName = `${base}${ext}`;
    setBusy(true);
    setError(null);
    try {
      await onConfirm(fileName);
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
        aria-labelledby="new-file-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="new-file-title">
          Новый файл
        </div>

        <div className="clone-modal-message">
          Папка: <b>{location}</b>
        </div>

        <label className="clone-modal-field">
          <span className="clone-modal-label">Имя</span>
          <input
            ref={inputRef}
            className="clone-modal-input"
            type="text"
            value={name}
            placeholder="description"
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void submit();
            }}
          />
        </label>

        <div className="clone-modal-field">
          <span className="clone-modal-label" id="new-file-ext-label">
            Тип
          </span>
          <div className="clone-select" ref={extRef}>
            <button
              type="button"
              className={`clone-select-trigger${extOpen ? " is-open" : ""}`}
              aria-haspopup="listbox"
              aria-expanded={extOpen}
              aria-labelledby="new-file-ext-label"
              onClick={() => setExtOpen((v) => !v)}
            >
              <span className="clone-select-value">
                <span className="clone-select-path">{selected.label}</span>
                <span className="clone-select-reason">{selected.ext}</span>
              </span>
              <span className="clone-select-chevron" aria-hidden>
                ▾
              </span>
            </button>
            {extOpen ? (
              <div className="clone-select-menu" role="listbox">
                {NEW_FILE_EXTENSION_OPTIONS.map((option) => (
                  <button
                    key={option.ext}
                    type="button"
                    role="option"
                    aria-selected={option.ext === ext}
                    className={`clone-select-option${option.ext === ext ? " is-active" : ""}`}
                    onClick={() => {
                      setExt(option.ext);
                      setExtOpen(false);
                    }}
                  >
                    <span className="clone-select-path">{option.label}</span>
                    <span className="clone-select-reason">{option.ext}</span>
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        </div>

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
