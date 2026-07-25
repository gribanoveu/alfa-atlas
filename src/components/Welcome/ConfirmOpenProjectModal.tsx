import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import type { DocsCandidate, ProbeResult } from "../../lib/project";
import "./CloneRepoModal.css";

type ConfirmOpenProjectModalProps = {
  probe: ProbeResult;
  onCancel: () => void;
  onConfirm: (docsRoot: string) => Promise<void>;
};

function folderName(path: string): string {
  return path.split(/[/\\]/).filter(Boolean).pop() ?? path;
}

export function ConfirmOpenProjectModal({
  probe,
  onCancel,
  onConfirm,
}: ConfirmOpenProjectModalProps) {
  const [docsRoot, setDocsRoot] = useState(
    probe.suggestedDocsRoot ?? probe.candidates[0]?.path ?? "",
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [candidatesOpen, setCandidatesOpen] = useState(false);
  const candidatesRef = useRef<HTMLDivElement>(null);

  const candidates: DocsCandidate[] = probe.candidates;
  const selectedCandidate = candidates.find((c) => c.path === docsRoot) ?? null;

  useEffect(() => {
    if (!candidatesOpen) return;

    const onPointerDown = (event: PointerEvent) => {
      if (!candidatesRef.current?.contains(event.target as Node)) {
        setCandidatesOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setCandidatesOpen(false);
    };

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [candidatesOpen]);

  const docsUnderRepo = useMemo(() => {
    if (!docsRoot) return false;
    const root = probe.root.replace(/[/\\]+$/, "");
    const docs = docsRoot.replace(/[/\\]+$/, "");
    return (
      docs === root ||
      docs.startsWith(`${root}/`) ||
      docs.startsWith(`${root}\\`)
    );
  }, [docsRoot, probe.root]);

  const pickDocsFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Корень документации",
      defaultPath: probe.root,
    });
    if (selected === null || Array.isArray(selected)) return;
    setDocsRoot(selected);
    setError(null);
  };

  const submit = async () => {
    if (!docsRoot.trim()) {
      setError("Укажите папку с документацией.");
      return;
    }
    if (!docsUnderRepo) {
      setError("Папка документации должна находиться внутри репозитория.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onConfirm(docsRoot.trim());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onClick={onCancel}
    >
      <div
        className="clone-modal"
        role="dialog"
        aria-labelledby="confirm-open-title"
        onClick={(event) => event.stopPropagation()}
        style={{ width: "min(520px, 100%)" }}
      >
        <div className="clone-modal-title" id="confirm-open-title">
          Корень документации
        </div>

        <div className="clone-modal-message">
          Репозиторий: <b>{folderName(probe.root)}</b>
          <div style={{ marginTop: 6, fontSize: 10.5, color: "var(--text-2)", wordBreak: "break-all" }}>
            {probe.root}
          </div>
        </div>

        <p style={{ margin: 0, fontSize: 12.5, color: "var(--text-1)", lineHeight: 1.45 }}>
          Рабочее пространство редактора — только документация. Подтвердите
          найденную папку или укажите её вручную.
        </p>

        {candidates.length > 0 ? (
          <div className="clone-modal-field">
            <span className="clone-modal-label" id="candidates-label">
              Найденные варианты
            </span>
            <div className="clone-select" ref={candidatesRef}>
              <button
                type="button"
                className={`clone-select-trigger${candidatesOpen ? " is-open" : ""}`}
                aria-haspopup="listbox"
                aria-expanded={candidatesOpen}
                aria-labelledby="candidates-label"
                onClick={() => setCandidatesOpen((isOpen) => !isOpen)}
              >
                <span className="clone-select-value">
                  {selectedCandidate ? (
                    <>
                      <span className="clone-select-path">
                        {selectedCandidate.relativePath}
                      </span>
                      <span className="clone-select-reason">
                        {selectedCandidate.reason}
                      </span>
                    </>
                  ) : (
                    <span className="clone-select-placeholder">Выберите…</span>
                  )}
                </span>
                <span className="clone-select-chevron" aria-hidden>
                  ▾
                </span>
              </button>
              {candidatesOpen ? (
                <div className="clone-select-menu" role="listbox">
                  {candidates.map((c) => {
                    const active = c.path === docsRoot;
                    return (
                      <button
                        key={c.path}
                        type="button"
                        role="option"
                        aria-selected={active}
                        className={`clone-select-option${active ? " is-active" : ""}`}
                        onClick={() => {
                          setDocsRoot(c.path);
                          setCandidatesOpen(false);
                          setError(null);
                        }}
                      >
                        <span className="clone-select-path">{c.relativePath}</span>
                        <span className="clone-select-reason">{c.reason}</span>
                      </button>
                    );
                  })}
                </div>
              ) : null}
            </div>
          </div>
        ) : (
          <div className="clone-modal-message">
            Автоматически найти папку документации не удалось. Укажите её
            вручную.
          </div>
        )}

        <label className="clone-modal-field">
          <span className="clone-modal-label">Корень документации</span>
          <div className="clone-modal-path-row">
            <input
              className="clone-modal-input"
              type="text"
              value={docsRoot}
              onChange={(event) => setDocsRoot(event.target.value)}
            />
            <button
              type="button"
              className="clone-modal-browse"
              onClick={() => void pickDocsFolder()}
            >
              Обзор…
            </button>
          </div>
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
            disabled={busy || !docsRoot.trim()}
          >
            Открыть
          </button>
        </div>
      </div>
    </div>
  );
}
