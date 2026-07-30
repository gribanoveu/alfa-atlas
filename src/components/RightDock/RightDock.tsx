import {
  FileText,
  GitCommitHorizontal,
  GitFork,
  Lightbulb,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import type { RightTool } from "../../hooks/useWorkspaceLayout";
import type {
  GitBranchInfo,
  GitDiffScope,
  GitFileStatus,
} from "../../lib/git";
import { PanelResizeHandle } from "../PanelResizeHandle/PanelResizeHandle";
import { HideIcon } from "../icons/HideIcon";
import { BranchesPanel } from "./BranchesPanel";
import { GitPanel } from "./GitPanel";
import { AsciiDocPanel } from "./AsciiDocPanel";
import "./RightDock.css";

const TOOL_DEFS: Record<
  RightTool,
  { label: string; empty: string; Icon: LucideIcon }
> = {
  assistant: {
    label: "Ассистент",
    empty: "Ассистент пока недоступен",
    Icon: Sparkles,
  },
  branches: {
    label: "Branches / Ветки",
    empty: "Нет открытого репозитория",
    Icon: GitFork,
  },
  git: {
    label: "Commit / Коммит",
    empty: "Нет изменений для отображения",
    Icon: GitCommitHorizontal,
  },
  suggestions: {
    label: "Подсказки",
    empty: "Нет активных подсказок",
    Icon: Lightbulb,
  },
  asciidoc: {
    label: "AsciiDoc",
    empty: "Библиотека блоков пуста",
    Icon: FileText,
  },
};

/** Stripe order: assistant on top, git tools grouped, editor tools below. */
const TOOL_STRIPE_GROUPS: RightTool[][] = [
  ["assistant"],
  ["branches", "git"],
  ["asciidoc"],
  ["suggestions"],
];

export type GitPanelViewProps = {
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
  onStageAll: (paths: string[]) => void;
  onUnstageAll: () => void;
  onCommit: () => void;
  onRefresh: () => void;
  onOpenFileDiff: (path: string, scope: GitDiffScope) => void;
  selectedDiff?: { path: string; scope: GitDiffScope } | null;
};

export type BranchesPanelViewProps = {
  currentBranch: string;
  branches: GitBranchInfo[];
  busy: boolean;
  error: string | null;
  onCheckout: (branch: GitBranchInfo) => void;
  onCreateBranch: (name: string) => void;
  onRefresh: () => void;
  onFetch: () => void;
};

type RightDockProps = {
  activeTool: RightTool | null;
  onToggleTool: (tool: RightTool) => void;
  onHide: () => void;
  onResize?: (delta: number) => void;
  onResizeEnd?: () => void;
  git?: GitPanelViewProps | null;
  branches?: BranchesPanelViewProps | null;
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
  branches,
  asciidoc,
}: RightDockProps) {
  const open = Boolean(activeTool);
  const active = activeTool ? TOOL_DEFS[activeTool] : undefined;

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

      {open && active && activeTool ? (
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
            {activeTool === "git" && git ? (
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
                onOpenFileDiff={git.onOpenFileDiff}
                selectedDiff={git.selectedDiff}
              />
            ) : activeTool === "asciidoc" && asciidoc ? (
              <AsciiDocPanel
                canInsert={asciidoc.canInsert}
                onInsert={asciidoc.onInsert}
              />
            ) : activeTool === "branches" && branches ? (
              <BranchesPanel
                currentBranch={branches.currentBranch}
                branches={branches.branches}
                busy={branches.busy}
                error={branches.error}
                onCheckout={branches.onCheckout}
                onCreateBranch={branches.onCreateBranch}
                onRefresh={branches.onRefresh}
                onFetch={branches.onFetch}
              />
            ) : (
              <div className="panel-empty">{active.empty}</div>
            )}
          </div>
        </div>
      ) : null}

      <nav className="tool-stripe" aria-label="Tool windows">
        {TOOL_STRIPE_GROUPS.map((group, groupIndex) => (
          <div key={group.join("-")} className="tool-stripe-group">
            {groupIndex > 0 ? <div className="tool-stripe-sep" role="separator" /> : null}
            {group.map((id) => {
              const { label, Icon } = TOOL_DEFS[id];
              return (
                <button
                  key={id}
                  type="button"
                  className={`tool-stripe-btn ${activeTool === id ? "active" : ""}`}
                  title={label}
                  aria-label={label}
                  aria-pressed={activeTool === id}
                  onClick={() => onToggleTool(id)}
                >
                  <Icon
                    className="tool-stripe-icon"
                    size={20}
                    strokeWidth={1.75}
                    aria-hidden
                  />
                </button>
              );
            })}
          </div>
        ))}
      </nav>
    </aside>
  );
}
