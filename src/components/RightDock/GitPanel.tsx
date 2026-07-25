import {
  ChevronDown,
  ChevronRight,
  Minus,
  Plus,
  RefreshCw,
} from "lucide-react";
import { useState, type ReactNode } from "react";
import type { GitFileStatus } from "../../lib/git";
import "./GitPanel.css";

type GitPanelProps = {
  staged: GitFileStatus[];
  unstaged: GitFileStatus[];
  jiraKey: string;
  onJiraKeyChange: (value: string) => void;
  description: string;
  onDescriptionChange: (value: string) => void;
  canCommit: boolean;
  busy: boolean;
  error: string | null;
  onStage: (path: string) => void;
  onUnstage: (path: string) => void;
  onStageAll: () => void;
  onUnstageAll: () => void;
  onCommit: () => void;
  onRefresh: () => void;
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

function splitPath(path: string): { name: string; dir: string } {
  const normalized = path.replace(/\\/g, "/");
  const idx = normalized.lastIndexOf("/");
  if (idx < 0) return { name: normalized, dir: "" };
  return {
    name: normalized.slice(idx + 1),
    dir: normalized.slice(0, idx + 1),
  };
}

function friendlyGitError(error: string): string {
  const lower = error.toLowerCase();
  if (
    lower.includes("user.name") ||
    lower.includes("user.email") ||
    lower.includes("missing identity")
  ) {
    return "Не заданы имя и email автора git (user.name / user.email). Попросите разработчика настроить или настройте сами в терминале.";
  }
  if (
    lower.includes("nothing staged") ||
    lower.includes("empty message") ||
    lower.includes("commit message is empty")
  ) {
    return "Нужно добавить файл в Stage и написать краткое описание.";
  }
  if (lower.startsWith("не удалось")) {
    return error;
  }
  return `Не удалось: ${error}`;
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
  children: ReactNode;
};

function GitGroup({
  title,
  hint,
  count,
  open,
  onToggle,
  headerAction,
  children,
}: GroupProps) {
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <section className="git-group">
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
  onAction: () => void;
};

function GitFileRow({
  file,
  actionLabel,
  actionIcon,
  busy,
  onAction,
}: FileRowProps) {
  const { name, dir } = splitPath(file.path);
  const statusTitle = statusLabel(file.status);
  return (
    <div className="git-file-row" title={file.path}>
      <span className="git-file-name">{name}</span>
      <span className="git-file-dir">{dir}</span>
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
          onClick={onAction}
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

export function GitPanel({
  staged,
  unstaged,
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
}: GitPanelProps) {
  const [changesOpen, setChangesOpen] = useState(true);
  const [stagedOpen, setStagedOpen] = useState(true);
  const emptyChanges = staged.length === 0 && unstaged.length === 0;

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
        <GitGroup
          title="Changes / Изменения"
          hint="Файлы, которые ещё не добавлены в коммит"
          count={unstaged.length}
          open={changesOpen}
          onToggle={() => setChangesOpen((v) => !v)}
          headerAction={
            unstaged.length > 0
              ? {
                  label: "Добавить все в Stage",
                  onClick: onStageAll,
                  icon: "plus",
                  disabled: busy,
                }
              : undefined
          }
        >
          {unstaged.length === 0 ? (
            <div className="git-empty">
              {emptyChanges
                ? "Пока нет несохранённых правок в git"
                : "Все изменения уже в Stage"}
            </div>
          ) : (
            unstaged.map((file) => (
              <GitFileRow
                key={`u:${file.path}`}
                file={file}
                actionLabel="Добавить в Stage"
                actionIcon="plus"
                busy={busy}
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
          onChange={(event) => onJiraKeyChange(event.target.value)}
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
