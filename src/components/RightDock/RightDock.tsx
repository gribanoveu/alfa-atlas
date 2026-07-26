import {
  FileText,
  GitBranch,
  History,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import type { RightTool } from "../../hooks/useWorkspaceLayout";
import type { GitCommitSummary, GitFileStatus } from "../../lib/git";
import { PanelResizeHandle } from "../PanelResizeHandle/PanelResizeHandle";
import { HideIcon } from "../icons/HideIcon";
import { CommitHistoryPanel } from "./CommitHistoryPanel";
import { GitPanel } from "./GitPanel";
import { AsciiDocPanel } from "./AsciiDocPanel";
import "./RightDock.css";

const TOOLS: {
  id: RightTool;
  label: string;
  empty: string;
  Icon: LucideIcon;
}[] = [
  {
    id: "assistant",
    label: "Ассистент",
    empty: "Ассистент пока недоступен",
    Icon: Sparkles,
  },
  {
    id: "asciidoc",
    label: "AsciiDoc",
    empty: "Библиотека блоков пуста",
    Icon: FileText,
  },
  {
    id: "git",
    label: "Git",
    empty: "Нет изменений для отображения",
    Icon: GitBranch,
  },
  {
    id: "gitHistory",
    label: "Commit history / История коммитов",
    empty: "Записей в истории пока нет",
    Icon: History,
  },
];

export type GitPanelViewProps = {
  staged: GitFileStatus[];
  unstaged: GitFileStatus[];
  commits: GitCommitSummary[];
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

type RightDockProps = {
  activeTool: RightTool | null;
  onToggleTool: (tool: RightTool) => void;
  onHide: () => void;
  onResize?: (delta: number) => void;
  onResizeEnd?: () => void;
  git?: GitPanelViewProps | null;
  asciidoc?: {
    canInsert: boolean;
    onInsert: (text: string) => void;
  } | null;
};

export function RightDock({
  activeTool,
  onToggleTool,
  onHide,
  onResize,
  onResizeEnd,
  git,
  asciidoc,
}: RightDockProps) {
  const open = Boolean(activeTool);
  const active = TOOLS.find((tool) => tool.id === activeTool);

  return (
    <aside className={`right-dock ${open ? "is-open" : "is-collapsed"}`}>
      {open && onResize ? (
        <PanelResizeHandle
          direction="horizontal"
          invert
          ariaLabel="Изменить ширину правой панели"
          onResize={onResize}
          onResizeEnd={onResizeEnd}
        />
      ) : null}

      {open && active ? (
        <div className="tool-window">
          <header className="tool-window-head">
            <span className="tool-window-title">{active.label}</span>
            <button
              type="button"
              className="tool-window-hide"
              onClick={onHide}
              title="Hide"
              aria-label={`Скрыть ${active.label}`}
            >
              <HideIcon />
            </button>
          </header>
          <div className="tool-window-body">
            {active.id === "git" && git ? (
              <GitPanel
                staged={git.staged}
                unstaged={git.unstaged}
                jiraKey={git.jiraKey}
                onJiraKeyChange={git.onJiraKeyChange}
                description={git.description}
                onDescriptionChange={git.onDescriptionChange}
                canCommit={git.canCommit}
                busy={git.busy}
                error={git.error}
                onStage={git.onStage}
                onUnstage={git.onUnstage}
                onStageAll={git.onStageAll}
                onUnstageAll={git.onUnstageAll}
                onCommit={git.onCommit}
                onRefresh={git.onRefresh}
              />
            ) : active.id === "gitHistory" && git ? (
              <CommitHistoryPanel
                commits={git.commits}
                busy={git.busy}
                error={git.error}
                onRefresh={git.onRefresh}
              />
            ) : active.id === "asciidoc" && asciidoc ? (
              <AsciiDocPanel
                canInsert={asciidoc.canInsert}
                onInsert={asciidoc.onInsert}
              />
            ) : (
              <div className="panel-empty">{active.empty}</div>
            )}
          </div>
        </div>
      ) : null}

      <nav className="tool-stripe" aria-label="Tool windows">
        {TOOLS.map(({ id, label, Icon }) => (
          <button
            key={id}
            type="button"
            className={`tool-stripe-btn ${activeTool === id ? "active" : ""}`}
            title={label}
            aria-label={label}
            aria-pressed={activeTool === id}
            onClick={() => onToggleTool(id)}
          >
            <Icon className="tool-stripe-icon" size={20} strokeWidth={1.75} aria-hidden />
          </button>
        ))}
      </nav>
    </aside>
  );
}
