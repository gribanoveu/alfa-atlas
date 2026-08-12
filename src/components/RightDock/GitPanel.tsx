import {
  ChevronDown,
  ChevronRight,
  Eye,
  Minus,
  Plus,
  RefreshCw,
  Trash2,
  Undo2,
} from "lucide-react";
import { useState, type ReactNode } from "react";
import type { GitDiffScope, GitFileStatus, GitStashEntry } from "../../lib/git";
import { friendlyGitError } from "../../lib/gitErrors";
import "./GitPanel.css";

type GitPanelProps = {
  staged: GitFileStatus[];
  unstaged: GitFileStatus[];
  conflicted: GitFileStatus[];
  mergeInProgress: boolean;
  jiraKey: string;
  onJiraKeyChange: (value: string) => void;
  description: string;
  onDescriptionChange: (value: string) => void;
  canCommit: boolean;
  busy: boolean;
  error: string | null;
  onStage: (path: string) => void;
  onUnstage: (path: string) => void;
  onStageAll: (paths: string[]) => void;
  onUnstageAll: () => void;
  onCommit: () => void;
  onRefresh: () => void;
  onOpenFileDiff: (path: string, scope: GitDiffScope) => void;
  onOpenConflict: (path: string) => void;
  onAbortMerge: () => void;
  onFinishMerge: () => void;
  selectedDiff?: { path: string; scope: GitDiffScope } | null;
  /** "Отложенные изменения" — docflow-managed auto-stash entries. */
  shelf: GitStashEntry[];
  shelfBusy: boolean;
  currentBranch: string | null;
  /** Entry currently mid-conflict-resolution (see App.tsx's pendingStashConflict) — shown as a badge instead of actions. */
  pendingShelfConflictId?: string | null;
  onRestoreShelfEntry: (entry: GitStashEntry) => void;
  onDiscardShelfEntry: (entry: GitStashEntry) => void;
  onPreviewShelfEntry: (entry: GitStashEntry) => void;
};

const STATUS_LABELS: Record<string, string> = {
  M: "Изменён",
  A: "Новый",
  D: "Удалён",
  R: "Переименован",
  "?": "Новый файл",
};

function statusLabel(status: string): string {
  return STATUS_LABELS[status] ?? status;
}

function statusClass(status: string): string {
  return status === "?" ? "untracked" : status;
}

function formatShelfTime(unixSeconds: number): string {
  const diffMin = Math.round((Date.now() - unixSeconds * 1000) / 60000);
  if (diffMin < 1) return "только что";
  if (diffMin < 60) return `${diffMin} мин назад`;
  const diffH = Math.round(diffMin / 60);
  if (diffH < 24) return `${diffH} ч назад`;
  const diffD = Math.round(diffH / 24);
  return `${diffD} дн назад`;
}

function filesWord(count: number): string {
  const mod10 = count % 10;
  const mod100 = count % 100;
  if (mod10 === 1 && mod100 !== 11) return "файл";
  if (mod10 >= 2 && mod10 <= 4 && (mod100 < 10 || mod100 >= 20)) return "файла";
  return "файлов";
}

function splitPath(path: string): { name: string; dir: string } {
  const normalized = path.replace(/\\/g, "/");
  const idx = normalized.lastIndexOf("/");
  if (idx < 0) return { name: normalized, dir: "" };
  return {
    name: normalized.slice(idx + 1),
    dir: normalized.slice(0, idx + 1),
  };
}

type GroupProps = {
  title: string;
  hint?: string;
  count?: number;
  open: boolean;
  onToggle: () => void;
  headerAction?: {
    label: string;
    onClick: () => void;
    icon: "plus" | "minus";
    disabled?: boolean;
  };
  tone?: "stage" | "conflict";
  children: ReactNode;
};

function GitGroup({
  title,
  hint,
  count,
  open,
  onToggle,
  headerAction,
  tone,
  children,
}: GroupProps) {
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <section className={`git-group${tone ? ` git-group-${tone}` : ""}`}>
      <div className="git-group-head">
        <button
          type="button"
          className="git-group-toggle"
          onClick={onToggle}
          aria-expanded={open}
          title={hint}
        >
          <Chevron className="git-group-chevron" size={14} aria-hidden />
          <span className="git-group-title">
            {title}
            {typeof count === "number" ? (
              <span className="git-group-count">({count})</span>
            ) : null}
          </span>
        </button>
        {headerAction ? (
          <button
            type="button"
            className="git-icon-btn"
            title={headerAction.label}
            aria-label={headerAction.label}
            disabled={headerAction.disabled}
            onClick={headerAction.onClick}
          >
            {headerAction.icon === "plus" ? (
              <Plus size={14} aria-hidden />
            ) : (
              <Minus size={14} aria-hidden />
            )}
          </button>
        ) : null}
      </div>
      {open ? <div className="git-group-body">{children}</div> : null}
    </section>
  );
}

type FileRowProps = {
  file: GitFileStatus;
  actionLabel: string;
  actionIcon: "plus" | "minus";
  busy: boolean;
  selected: boolean;
  onOpenDiff: () => void;
  onAction: () => void;
};

function GitFileRow({
  file,
  actionLabel,
  actionIcon,
  busy,
  selected,
  onOpenDiff,
  onAction,
}: FileRowProps) {
  const { name, dir } = splitPath(file.path);
  const statusTitle = statusLabel(file.status);
  return (
    <div
      className={`git-file-row${selected ? " git-file-row-selected" : ""}`}
      title={file.path}
    >
      <button
        type="button"
        className="git-file-main"
        disabled={busy}
        onClick={onOpenDiff}
        aria-label={`Показать diff: ${file.path}`}
      >
        <span className="git-file-name">{name}</span>
        <span className="git-file-dir">{dir}</span>
      </button>
      <span className="git-file-trailing">
        <span
          className={`git-status git-status-${statusClass(file.status)}`}
          title={statusTitle}
          aria-label={statusTitle}
        >
          {file.status}
        </span>
        <button
          type="button"
          className="git-icon-btn git-file-action"
          title={actionLabel}
          aria-label={`${actionLabel}: ${file.path}`}
          disabled={busy}
          onClick={(event) => {
            event.stopPropagation();
            onAction();
          }}
        >
          {actionIcon === "plus" ? (
            <Plus size={14} aria-hidden />
          ) : (
            <Minus size={14} aria-hidden />
          )}
        </button>
      </span>
    </div>
  );
}

function GitConflictFileRow({
  file,
  busy,
  onOpen,
}: {
  file: GitFileStatus;
  busy: boolean;
  onOpen: () => void;
}) {
  const { name, dir } = splitPath(file.path);
  return (
    <button
      type="button"
      className="git-file-row git-conflict-file-row"
      disabled={busy}
      onClick={onOpen}
      title={file.path}
      aria-label={`Разрешить конфликт: ${file.path}`}
    >
      <span className="git-file-main">
        <span className="git-file-name">{name}</span>
        <span className="git-file-dir">{dir}</span>
      </span>
      <span className="git-status git-status-U" title="Конфликт" aria-label="Конфликт">
        U
      </span>
    </button>
  );
}

function GitStashRow({
  entry,
  busy,
  currentBranch,
  isPendingConflict,
  onRestore,
  onDiscard,
  onPreview,
}: {
  entry: GitStashEntry;
  busy: boolean;
  currentBranch: string | null;
  isPendingConflict: boolean;
  onRestore: () => void;
  onDiscard: () => void;
  onPreview: () => void;
}) {
  const onOtherBranch = entry.branch !== currentBranch;
  return (
    <div className="git-file-row git-stash-row" title={entry.branch}>
      <button
        type="button"
        className="git-file-main"
        disabled={busy}
        onClick={onPreview}
        aria-label={`Просмотреть отложенные изменения: ${entry.branch}`}
      >
        <span className="git-file-name">{entry.branch}</span>
        <span className="git-file-dir">
          {formatShelfTime(entry.createdAt)} · {entry.filesChanged}{" "}
          {filesWord(entry.filesChanged)}
        </span>
      </button>
      {isPendingConflict ? (
        <span className="git-stash-conflict-badge" title="Конфликт при восстановлении — разрешите его в панели выше">
          конфликт
        </span>
      ) : (
        <span className="git-file-trailing">
          <button
            type="button"
            className="git-icon-btn"
            title={
              onOtherBranch
                ? `Переключитесь на «${entry.branch}», чтобы восстановить`
                : "Восстановить"
            }
            aria-label={`Восстановить отложенные изменения: ${entry.branch}`}
            disabled={busy || onOtherBranch}
            onClick={(event) => {
              event.stopPropagation();
              onRestore();
            }}
          >
            <Undo2 size={14} aria-hidden />
          </button>
          <button
            type="button"
            className="git-icon-btn"
            title="Просмотреть"
            aria-label={`Просмотреть отложенные изменения: ${entry.branch}`}
            disabled={busy}
            onClick={(event) => {
              event.stopPropagation();
              onPreview();
            }}
          >
            <Eye size={14} aria-hidden />
          </button>
          <button
            type="button"
            className="git-icon-btn git-stash-discard"
            title="Удалить"
            aria-label={`Удалить отложенные изменения: ${entry.branch}`}
            disabled={busy}
            onClick={(event) => {
              event.stopPropagation();
              onDiscard();
            }}
          >
            <Trash2 size={14} aria-hidden />
          </button>
        </span>
      )}
    </div>
  );
}

export function GitPanel({
  staged,
  unstaged,
  conflicted,
  mergeInProgress,
  jiraKey,
  onJiraKeyChange,
  description,
  onDescriptionChange,
  canCommit,
  busy,
  error,
  onStage,
  onUnstage,
  onStageAll,
  onUnstageAll,
  onCommit,
  onRefresh,
  onOpenFileDiff,
  onOpenConflict,
  onAbortMerge,
  onFinishMerge,
  selectedDiff = null,
  shelf,
  shelfBusy,
  currentBranch,
  pendingShelfConflictId = null,
  onRestoreShelfEntry,
  onDiscardShelfEntry,
  onPreviewShelfEntry,
}: GitPanelProps) {
  const [changesOpen, setChangesOpen] = useState(true);
  const [newFilesOpen, setNewFilesOpen] = useState(true);
  const [stagedOpen, setStagedOpen] = useState(true);
  const [conflictsOpen, setConflictsOpen] = useState(true);
  const [shelfOpen, setShelfOpen] = useState(true);
  const emptyChanges = staged.length === 0 && unstaged.length === 0;
  const modifiedUnstaged = unstaged.filter((file) => file.status !== "?");
  const newFiles = unstaged.filter((file) => file.status === "?");

  return (
    <div className="git-panel">
      <div className="git-panel-toolbar">
        <button
          type="button"
          className="git-icon-btn"
          title="Обновить список"
          aria-label="Обновить список"
          disabled={busy}
          onClick={onRefresh}
        >
          <RefreshCw size={14} aria-hidden />
        </button>
      </div>

      <div className="git-panel-scroll">
        {mergeInProgress || conflicted.length > 0 ? (
          <>
            <GitGroup
              title="Конфликты слияния"
              hint="Файлы с неразрешёнными конфликтами — откройте и разрешите каждый"
              count={conflicted.length}
              open={conflictsOpen}
              onToggle={() => setConflictsOpen((v) => !v)}
              tone="conflict"
            >
              {conflicted.length === 0 ? (
                <div className="git-conflict-resolved-banner">
                  Все конфликты разрешены, но слияние не завершилось автоматически.
                  <button
                    type="button"
                    className="git-conflict-finish-btn"
                    disabled={busy}
                    onClick={onFinishMerge}
                  >
                    Завершить слияние
                  </button>
                </div>
              ) : (
                conflicted.map((file) => (
                  <GitConflictFileRow
                    key={`c:${file.path}`}
                    file={file}
                    busy={busy}
                    onOpen={() => onOpenConflict(file.path)}
                  />
                ))
              )}
              <div className="git-conflict-abort-row">
                <button
                  type="button"
                  className="git-conflict-abort-btn"
                  disabled={busy}
                  onClick={onAbortMerge}
                  title="Откатить слияние и вернуть файлы к состоянию до pull"
                >
                  Отменить слияние
                </button>
              </div>
            </GitGroup>
            <div className="git-section-divider" role="separator" />
          </>
        ) : null}

        {shelf.length > 0 ? (
          <>
            <GitGroup
              title="Отложенные изменения"
              hint="Изменения, отложенные автоматически при переключении веток — восстанавливаются при возврате на исходную ветку"
              count={shelf.length}
              open={shelfOpen}
              onToggle={() => setShelfOpen((v) => !v)}
            >
              {shelf.map((entry) => (
                <GitStashRow
                  key={entry.id}
                  entry={entry}
                  busy={shelfBusy}
                  currentBranch={currentBranch}
                  isPendingConflict={pendingShelfConflictId === entry.id}
                  onRestore={() => onRestoreShelfEntry(entry)}
                  onDiscard={() => onDiscardShelfEntry(entry)}
                  onPreview={() => onPreviewShelfEntry(entry)}
                />
              ))}
            </GitGroup>
            <div className="git-section-divider" role="separator" />
          </>
        ) : null}

        <GitGroup
          title="Changes / Изменения"
          hint="Изменённые файлы, которые ещё не добавлены в коммит"
          count={modifiedUnstaged.length}
          open={changesOpen}
          onToggle={() => setChangesOpen((v) => !v)}
          headerAction={
            modifiedUnstaged.length > 0
              ? {
                  label: "Добавить все в Stage",
                  onClick: () =>
                    onStageAll(modifiedUnstaged.map((f) => f.path)),
                  icon: "plus",
                  disabled: busy,
                }
              : undefined
          }
        >
          {modifiedUnstaged.length === 0 ? (
            <div className="git-empty">
              {emptyChanges
                ? "Пока нет несохранённых правок в git"
                : "Нет изменённых файлов"}
            </div>
          ) : (
            modifiedUnstaged.map((file) => (
              <GitFileRow
                key={`u:${file.path}`}
                file={file}
                actionLabel="Добавить в Stage"
                actionIcon="plus"
                busy={busy}
                selected={
                  selectedDiff?.path === file.path &&
                  selectedDiff.scope === "unstaged"
                }
                onOpenDiff={() => onOpenFileDiff(file.path, "unstaged")}
                onAction={() => onStage(file.path)}
              />
            ))
          )}
        </GitGroup>

        <div className="git-section-divider" role="separator" />

        <GitGroup
          title="New files / Новые файлы"
          hint="Новые файлы, которые ещё не отслеживаются git"
          count={newFiles.length}
          open={newFilesOpen}
          onToggle={() => setNewFilesOpen((v) => !v)}
          headerAction={
            newFiles.length > 0
              ? {
                  label: "Добавить все в Stage",
                  onClick: () => onStageAll(newFiles.map((f) => f.path)),
                  icon: "plus",
                  disabled: busy,
                }
              : undefined
          }
        >
          {newFiles.length === 0 ? (
            <div className="git-empty">
              {emptyChanges ? "Пока нет новых файлов" : "Нет новых файлов"}
            </div>
          ) : (
            newFiles.map((file) => (
              <GitFileRow
                key={`u:${file.path}`}
                file={file}
                actionLabel="Добавить в Stage"
                actionIcon="plus"
                busy={busy}
                selected={
                  selectedDiff?.path === file.path &&
                  selectedDiff.scope === "unstaged"
                }
                onOpenDiff={() => onOpenFileDiff(file.path, "unstaged")}
                onAction={() => onStage(file.path)}
              />
            ))
          )}
        </GitGroup>

        <div className="git-section-divider" role="separator" />

        <GitGroup
          title="Stage / Добавлены в коммит"
          hint="Войдут в следующий Commit"
          count={staged.length}
          open={stagedOpen}
          onToggle={() => setStagedOpen((v) => !v)}
          tone="stage"
          headerAction={
            staged.length > 0
              ? {
                  label: "Убрать все из Stage",
                  onClick: onUnstageAll,
                  icon: "minus",
                  disabled: busy,
                }
              : undefined
          }
        >
          {staged.length === 0 ? (
            <div className="git-empty">Пока ничего не добавлено в Stage</div>
          ) : (
            staged.map((file) => (
              <GitFileRow
                key={`s:${file.path}`}
                file={file}
                actionLabel="Убрать из Stage"
                actionIcon="minus"
                busy={busy}
                selected={
                  selectedDiff?.path === file.path &&
                  selectedDiff.scope === "staged"
                }
                onOpenDiff={() => onOpenFileDiff(file.path, "staged")}
                onAction={() => onUnstage(file.path)}
              />
            ))
          )}
        </GitGroup>
      </div>

      <div className="git-commit-dock">
        <input
          className="git-jira-input"
          type="text"
          placeholder="Номер задачи Jira, нап. JIRA-123, пусто если нет задачи"
          value={jiraKey}
          disabled={busy}
          spellCheck={false}
          onChange={(event) =>
            onJiraKeyChange(event.target.value.toUpperCase())
          }
        />
        <textarea
          className="git-commit-message"
          rows={3}
          placeholder="Краткое описание изменений"
          value={description}
          disabled={busy}
          onChange={(event) => onDescriptionChange(event.target.value)}
        />
        <div className="git-commit-actions">
          <button
            type="button"
            className="git-commit-btn"
            disabled={!canCommit}
            onClick={onCommit}
            title={
              canCommit
                ? "Создать коммит: doc(JIRA): описание"
                : "Нужны Stage и краткое описание"
            }
          >
            Commit
          </button>
        </div>
        {error ? (
          <div className="git-panel-error">{friendlyGitError(error)}</div>
        ) : null}
      </div>
    </div>
  );
}
