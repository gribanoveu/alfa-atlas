import { DiffEditor, type DiffOnMount } from "@monaco-editor/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useGitFileDiff } from "../../hooks/useGitFileDiff";
import type { GitDiffScope, GitFileDiff, GitFileStatus } from "../../lib/git";
import { monacoLanguageFor } from "../../lib/supportedFiles";
import { ATLAS_DARK_THEME_ID } from "../../monaco/asciidocLanguage";
import { DiscardChangesConfirmModal } from "./DiscardChangesConfirmModal";
import "../Welcome/CloneRepoModal.css";
import "./GitFileDiffModal.css";

type GitFileDiffModalProps = {
  target: { file: GitFileStatus; scope: GitDiffScope };
  busy: boolean;
  editorFontSizePx: number;
  onClose: () => void;
  onLoadDiff: (path: string, scope: GitDiffScope) => Promise<GitFileDiff | null>;
  onDiscard: (path: string) => Promise<boolean>;
  onSaveContent: (
    path: string,
    scope: GitDiffScope,
    content: string,
  ) => Promise<boolean>;
};

export function GitFileDiffModal({
  target,
  busy,
  editorFontSizePx,
  onClose,
  onLoadDiff,
  onDiscard,
  onSaveContent,
}: GitFileDiffModalProps) {
  const { diff, loading, error, discarding, saving, discard, save } = useGitFileDiff({
    target,
    onLoadDiff,
    onDiscard,
    onSaveContent,
  });
  const [confirmingDiscard, setConfirmingDiscard] = useState(false);
  const editorWrapRef = useRef<HTMLDivElement>(null);
  const diffEditorRef = useRef<Parameters<DiffOnMount>[0] | null>(null);

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
      if (event.key !== "Escape" || busy || discarding || saving) return;
      if (confirmingDiscard) {
        setConfirmingDiscard(false);
        return;
      }
      onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [busy, discarding, saving, confirmingDiscard, onClose]);

  const handleDiscard = async () => {
    setConfirmingDiscard(false);
    if (await discard()) onClose();
  };

  const handleSave = async () => {
    const editor = diffEditorRef.current;
    if (!editor) return;
    if ((await save(editor.getModifiedEditor().getValue())) === "gone") onClose();
  };

  const language = monacoLanguageFor(target.file.path);
  const actionBusy = busy || discarding || saving;

  // Stable reference: @monaco-editor/react calls diffEditor.updateOptions()
  // on every prop change after mount, which is otherwise on every render
  // if this object were an inline literal — repeatedly re-applying the
  // scrollbar visibility option was flipping the left pane's scrollbar back
  // to visible. Only editorFontSizePx ever actually changes here.
  const diffEditorOptions = useMemo(
    () => ({
      readOnly: false,
      originalEditable: false,
      renderSideBySide: true,
      renderMarginRevertIcon: true,
      // Monaco's newer hover-based "gutter menu" takes over the revert UI
      // by default and hides the classic always-visible revert arrows set
      // via renderMarginRevertIcon above — switch it off to get them back.
      renderGutterMenu: false,
      // Keep both panes' gutter width identical (the modified side needs
      // glyph margin space for the revert arrows anyway) so wrapped lines
      // stay aligned and both sides scroll in lockstep.
      glyphMargin: true,
      automaticLayout: true,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      // Keep both sides horizontally scrollable instead of wrapping long
      // lines differently when their available widths do not match.
      wordWrap: "off" as const,
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
              <span className="git-diff-hint">
                Нажмите на стрелку слева от изменённого блока в правой панели,
                чтобы откатить только его
              </span>
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
              theme={ATLAS_DARK_THEME_ID}
              language={language}
              original={diff.original}
              modified={diff.modified}
              onMount={(editor) => {
                diffEditorRef.current = editor;
              }}
              options={diffEditorOptions}
            />
          ) : null}
        </div>

        <div className="clone-modal-actions git-diff-actions">
          <button
            type="button"
            className="clone-modal-btn git-diff-discard-btn"
            disabled={actionBusy || loading || diff?.isBinary === true}
            onClick={() => setConfirmingDiscard(true)}
            title={
              target.file.status === "?"
                ? "Удалить файл — он не отслеживается git"
                : "Вернуть файл к последнему коммиту (HEAD)"
            }
          >
            {target.file.status === "?" ? "Удалить файл" : "Отменить изменения"}
          </button>
          <div className="git-diff-actions-right">
            <button
              type="button"
              className="clone-modal-btn"
              disabled={actionBusy}
              onClick={onClose}
            >
              Закрыть
            </button>
            <button
              type="button"
              className="clone-modal-btn primary"
              disabled={actionBusy || loading || diff?.isBinary === true}
              onClick={() => void handleSave()}
              title="Сохранить содержимое из окна сравнения (после отката отдельных изменений)"
            >
              {saving ? "Сохранение…" : "Сохранить"}
            </button>
          </div>
        </div>
      </div>

      {confirmingDiscard ? (
        <DiscardChangesConfirmModal
          path={target.file.path}
          isUntracked={target.file.status === "?"}
          busy={discarding}
          onCancel={() => setConfirmingDiscard(false)}
          onConfirm={() => void handleDiscard()}
        />
      ) : null}
    </div>
  );
}

function statusClass(status: string): string {
  return status === "?" ? "untracked" : status;
}
