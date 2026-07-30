import { DiffEditor } from "@monaco-editor/react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { GitFileDiff, GitFileStatus } from "../../lib/git";
import { monacoLanguageFor } from "../../lib/supportedFiles";
import "../Welcome/CloneRepoModal.css";
import "./GitFileDiffModal.css";

type GitCommitFileDiffModalProps = {
  commitHash: string;
  file: GitFileStatus;
  editorFontSizePx: number;
  onClose: () => void;
  onLoadDiff: (commitHash: string, path: string) => Promise<GitFileDiff | null>;
};

export function GitCommitFileDiffModal({
  commitHash,
  file,
  editorFontSizePx,
  onClose,
  onLoadDiff,
}: GitCommitFileDiffModalProps) {
  const [diff, setDiff] = useState<GitFileDiff | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const editorWrapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setDiff(null);

    void onLoadDiff(commitHash, file.path).then((result) => {
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
  }, [onLoadDiff, commitHash, file.path]);

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
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const language = monacoLanguageFor(file.path);

  const diffEditorOptions = useMemo(
    () => ({
      readOnly: true,
      originalEditable: false,
      renderSideBySide: true,
      automaticLayout: true,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      wordWrap: "on" as const,
      fontFamily: "'JetBrains Mono', ui-monospace, monospace",
      fontSize: editorFontSizePx,
      renderOverviewRuler: false,
      scrollbar: {
        useShadows: false,
        vertical: "hidden" as const,
        horizontal: "hidden" as const,
        verticalScrollbarSize: 0,
        horizontalScrollbarSize: 0,
        handleMouseWheel: true,
      },
    }),
    [editorFontSizePx],
  );

  return (
    <div
      className="clone-modal-backdrop git-diff-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="clone-modal git-diff-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="git-commit-diff-title"
      >
        <div className="git-diff-head">
          <div>
            <div className="clone-modal-title" id="git-commit-diff-title">
              {file.path}
            </div>
            <div className="git-diff-meta">
              <span className={`git-status git-status-${file.status}`}>
                {file.status}
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
              options={diffEditorOptions}
            />
          ) : null}
        </div>

        <div className="clone-modal-actions">
          <button type="button" className="clone-modal-btn" onClick={onClose}>
            Закрыть
          </button>
        </div>
      </div>
    </div>
  );
}
