import { useEffect, useRef, useState } from "react";
import "../Welcome/CloneRepoModal.css";

type NewFolderModalProps = {
  parentPath: string;
  onCancel: () => void;
  onConfirm: (folderName: string, useRestEndpointTemplate: boolean) => Promise<void>;
};

const TEMPLATE_OPTIONS: { value: boolean; label: string }[] = [
  { value: false, label: "Нет" },
  { value: true, label: "Документация на REST метод" },
];

export function NewFolderModal({
  parentPath,
  onCancel,
  onConfirm,
}: NewFolderModalProps) {
  const [name, setName] = useState("");
  const [useRestTemplate, setUseRestTemplate] = useState(false);
  const [templateOpen, setTemplateOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const templateRef = useRef<HTMLDivElement>(null);

  const selectedTemplate =
    TEMPLATE_OPTIONS.find((o) => o.value === useRestTemplate) ??
    TEMPLATE_OPTIONS[0];

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    if (!templateOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!templateRef.current?.contains(event.target as Node)) {
        setTemplateOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setTemplateOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [templateOpen]);

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
      await onConfirm(trimmed, useRestTemplate);
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

        <div className="clone-modal-field">
          <span className="clone-modal-label" id="new-folder-template-label">
            Шаблон
          </span>
          <div className="clone-select" ref={templateRef}>
            <button
              type="button"
              className={`clone-select-trigger${templateOpen ? " is-open" : ""}`}
              aria-haspopup="listbox"
              aria-expanded={templateOpen}
              aria-labelledby="new-folder-template-label"
              onClick={() => setTemplateOpen((v) => !v)}
            >
              <span className="clone-select-value">
                <span className="clone-select-path">
                  {selectedTemplate.label}
                </span>
              </span>
              <span className="clone-select-chevron" aria-hidden>
                ▾
              </span>
            </button>
            {templateOpen ? (
              <div className="clone-select-menu" role="listbox">
                {TEMPLATE_OPTIONS.map((option) => (
                  <button
                    key={option.label}
                    type="button"
                    role="option"
                    aria-selected={option.value === useRestTemplate}
                    className={`clone-select-option${option.value === useRestTemplate ? " is-active" : ""}`}
                    onClick={() => {
                      setUseRestTemplate(option.value);
                      setTemplateOpen(false);
                    }}
                  >
                    <span className="clone-select-path">{option.label}</span>
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        </div>

        {useRestTemplate ? (
          <div className="clone-modal-message">
            Будет создана папка «{name.trim() || "…"}» с файлами{" "}
            <b>{(name.trim() || "methodName") + ".adoc"}</b>, request.adoc,
            response.adoc и {(name.trim() || "methodName") + ".puml"}.
          </div>
        ) : null}

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
