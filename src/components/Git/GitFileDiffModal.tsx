import { DiffEditor } from "@monaco-editor/react";
import { useEffect, useRef, useState } from "react";
import type { GitDiffScope, GitFileDiff, GitFileStatus } from "../../lib/git";
import { monacoLanguageFor } from "../../lib/supportedFiles";
import "../Welcome/CloneRepoModal.css";
import "./GitFileDiffModal.css";

type GitFileDiffModalProps = {
  target: { file: GitFileStatus; scope: GitDiffScope };
  busy: boolean;
  editorFontSizePx: number;
  onClose: () => void;
  onLoadDiff: (path: string, scope: GitDiffScope) => Promise<GitFileDiff | null>;
  onDiscard: (path: string) => Promise<boolean>;
};

export function GitFileDiffModal({
  target,
  busy,
  editorFontSizePx,
  onClose,
  onLoadDiff,
  onDiscard,
}: GitFileDiffModalProps) {
  const [diff, setDiff] = useState<GitFileDiff | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [discarding, setDiscarding] = useState(false);
  const editorWrapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setDiff(null);

    void onLoadDiff(target.file.path, target.scope).then((result) => {
      if (cancelled) return;
      if (!result) {
        setError("Не удалось загрузить diff");
      } else {
        setDiff(result);
      }
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [onLoadDiff, target.file.path, target.scope]);

  useEffect(() => {
    const el = editorWrapRef.current;
    if (!el || !diff || diff.isBinary) return;

    const observer = new ResizeObserver(() => {
      window.dispatchEvent(new Event("resize"));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [diff]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy && !discarding) onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [busy, discarding, onClose]);

  const handleDiscard = async () => {
    const confirmed = window.confirm(
      `Отменить все изменения в «${target.file.path}» и вернуть файл к последнему коммиту?`,
    );
    if (!confirmed) return;

    setDiscarding(true);
    setError(null);
    try {
      const ok = await onDiscard(target.file.path);
      if (ok) onClose();
      else setError("Не удалось отменить изменения");
    } finally {
      setDiscarding(false);
    }
  };

  const language = monacoLanguageFor(target.file.path);
  const actionBusy = busy || discarding;

  return (
    <div
      className="clone-modal-backdrop git-diff-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (!actionBusy && event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="clone-modal git-diff-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="git-diff-title"
      >
        <div className="git-diff-head">
          <div>
            <div className="clone-modal-title" id="git-diff-title">
              {target.file.path}
            </div>
            <div className="git-diff-meta">
              <span className={`git-status git-status-${statusClass(target.file.status)}`}>
                {target.file.status}
              </span>
              {diff ? (
                <span className="git-diff-labels">
                  {diff.originalLabel} ↔ {diff.modifiedLabel}
                </span>
              ) : null}
            </div>
          </div>
          <button
            type="button"
            className="git-diff-close"
            aria-label="Закрыть"
            disabled={actionBusy}
            onClick={onClose}
          >
            ×
          </button>
        </div>

        <div className="git-diff-body" ref={editorWrapRef}>
          {loading ? (
            <div className="git-diff-placeholder">Загрузка diff…</div>
          ) : error ? (
            <div className="git-diff-placeholder git-diff-error">{error}</div>
          ) : diff?.isBinary ? (
            <div className="git-diff-placeholder">
              Бинарный файл — diff недоступен
            </div>
          ) : diff ? (
            <DiffEditor
              height="100%"
              theme="vs-dark"
              language={language}
              original={diff.original}
              modified={diff.modified}
              options={{
                readOnly: true,
                renderSideBySide: true,
                automaticLayout: true,
                minimap: { enabled: false },
                scrollBeyondLastLine: false,
                wordWrap: "on",
                fontFamily: "'JetBrains Mono', ui-monospace, monospace",
                fontSize: editorFontSizePx,
                renderOverviewRuler: false,
              }}
            />
          ) : null}
        </div>

        <div className="clone-modal-actions git-diff-actions">
          <button
            type="button"
            className="clone-modal-btn git-diff-discard-btn"
            disabled={actionBusy || loading || diff?.isBinary === true}
            onClick={() => void handleDiscard()}
            title="Вернуть файл к последнему коммиту (HEAD)"
          >
            Отменить изменения
          </button>
          <button
            type="button"
            className="clone-modal-btn clone-modal-btn-primary"
            disabled={actionBusy}
            onClick={onClose}
          >
            Закрыть
          </button>
        </div>
      </div>
    </div>
  );
}

function statusClass(status: string): string {
  return status === "?" ? "untracked" : status;
}
