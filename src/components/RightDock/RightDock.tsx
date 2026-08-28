import { useState } from "react";
import {
  FileText,
  GitCommitHorizontal,
  GitFork,
  Lightbulb,
  Sparkles,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import type { RightTool } from "../../hooks/useWorkspaceLayout";
import type { UtilityId } from "../../data/utilities";
import type { ArtifactKind } from "../../lib/artifacts";
import type {
  GitBranchInfo,
  GitDiffScope,
  GitFileStatus,
  GitStashEntry,
} from "../../lib/git";
import type { GitActionLogEntry } from "../../lib/gitActionLog";
import type { SpecsRepoInfo } from "../../lib/openapi";
import type { UpdatedReference } from "../../lib/project";
import type { ConversationMode } from "../../lib/aiTools";
import { PanelResizeHandle } from "../PanelResizeHandle/PanelResizeHandle";
import { HideIcon } from "../icons/HideIcon";
import { AssistantPanel } from "./AssistantPanel";
import { BranchesPanel } from "./BranchesPanel";
import { GitPanel } from "./GitPanel";
import { AsciiDocPanel } from "./AsciiDocPanel";
import { UtilitiesPanel } from "./UtilitiesPanel";
import { NotificationsPanel } from "./NotificationsPanel";
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
    label: "Уведомления",
    empty: "Нет активных уведомлений",
    Icon: Lightbulb,
  },
  asciidoc: {
    label: "AsciiDoc",
    empty: "Библиотека блоков пуста",
    Icon: FileText,
  },
  utilities: {
    label: "Утилиты",
    empty: "Нет открытого репозитория",
    Icon: Wrench,
  },
};

/** Stripe order: notifications + assistant grouped on top, then git tools, then editor tools. */
const TOOL_STRIPE_GROUPS: RightTool[][] = [
  ["suggestions", "assistant"],
  ["branches", "git"],
  ["asciidoc", "utilities"],
];

export type GitPanelViewProps = {
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
  shelf: GitStashEntry[];
  shelfBusy: boolean;
  currentBranch: string | null;
  pendingShelfConflictId?: string | null;
  onRestoreShelfEntry: (entry: GitStashEntry) => void;
  onDiscardShelfEntry: (entry: GitStashEntry) => void;
  onPreviewShelfEntry: (entry: GitStashEntry) => void;
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
  onDelete: (branch: GitBranchInfo) => void;
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
  utilities?: {
    onOpen: (id: UtilityId) => void;
    activeId: UtilityId | null;
    onNewArtifact: (kind: ArtifactKind) => void;
    onOpenArtifacts: () => void;
  } | null;
  assistant?: {
    onOpenSettings: () => void;
    specsRepoInfo: SpecsRepoInfo | null;
    docsRoot: string;
    onFileWritten: (info: { tool: string; path: string }) => void;
    onFileMoved: (info: { from: string; to: string; updatedFiles: UpdatedReference[] }) => void;
    repoRoot: string | null;
    activeFilePath: string | null;
  } | null;
  gitActionLog?: {
    entries: GitActionLogEntry[];
    busy: boolean;
    onUndo: (entry: GitActionLogEntry) => void;
  } | null;
  /** «Добавить в чат» из редактора — запрос на вставку выделенного фрагмента
   * в черновик ввода чата ассистента; потребляется в AssistantConversation. */
  chatInsertRequest?: {
    id: number;
    text: string;
    filePath: string | null;
  } | null;
  /** Вызывается сразу после того, как запрос выше вставлен в черновик —
   * чистит его в App, чтобы перемонтирование AssistantConversation (смена
   * чата, переключение инструментов дока) не вставило его повторно. */
  onChatInsertHandled?: () => void;
  /** Editor context action — canned prompt to send immediately. */
  assistantSendRequest?: {
    id: number;
    text: string;
    conversationMode?: ConversationMode;
  } | null;
  onAssistantSendHandled?: () => void;
  assistantDraftRequest?: {
    id: number;
    text: string;
    conversationMode?: ConversationMode;
  } | null;
  onAssistantDraftHandled?: () => void;
};

function ToolWindowHeader({ label, onHide }: { label: string; onHide: () => void }) {
  return (
    <header className="tool-window-head">
      <span className="tool-window-title">{label}</span>
      <button
        type="button"
        className="tool-window-hide"
        onClick={onHide}
        title="Hide"
        aria-label={`Скрыть ${label}`}
      >
        <HideIcon />
      </button>
    </header>
  );
}

export function RightDock({
  activeTool,
  onToggleTool,
  onHide,
  onResize,
  onResizeEnd,
  git,
  branches,
  asciidoc,
  utilities,
  assistant,
  gitActionLog,
  chatInsertRequest,
  onChatInsertHandled,
  assistantSendRequest,
  onAssistantSendHandled,
  assistantDraftRequest,
  onAssistantDraftHandled,
}: RightDockProps) {
  const open = Boolean(activeTool);
  const active = activeTool ? TOOL_DEFS[activeTool] : undefined;

  // Latches true the first time the assistant tool is opened (while a repo
  // is actually open) and never resets — closing the panel or switching to
  // another dock tool only hides the assistant window via CSS from then on,
  // instead of unmounting it. Unmounting would destroy `AssistantPanel`'s
  // in-flight state: an unsent draft, streaming text, or (worst case) a
  // `Promise` waiting on the user to answer a clarifying question, which
  // would then be stranded forever with no UI left to resolve it. If
  // `assistant` later goes back to `null` (the repo was closed), the window
  // unmounts anyway below since there's no repo left for that state to mean
  // anything — reopening a repo mounts a fresh instance for it.
  const [assistantMounted, setAssistantMounted] = useState(activeTool === "assistant" && Boolean(assistant));
  if (activeTool === "assistant" && assistant && !assistantMounted) {
    setAssistantMounted(true);
  }

  const showAssistant = open && activeTool === "assistant";
  const showOther = open && active && activeTool && activeTool !== "assistant";

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

      {showOther ? (
        <div className="tool-window">
          <ToolWindowHeader label={active.label} onHide={onHide} />
          <div className="tool-window-body">
            {activeTool === "git" && git ? (
              <GitPanel
                staged={git.staged}
                unstaged={git.unstaged}
                conflicted={git.conflicted}
                mergeInProgress={git.mergeInProgress}
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
                onOpenConflict={git.onOpenConflict}
                onAbortMerge={git.onAbortMerge}
                onFinishMerge={git.onFinishMerge}
                selectedDiff={git.selectedDiff}
                shelf={git.shelf}
                shelfBusy={git.shelfBusy}
                currentBranch={git.currentBranch}
                pendingShelfConflictId={git.pendingShelfConflictId}
                onRestoreShelfEntry={git.onRestoreShelfEntry}
                onDiscardShelfEntry={git.onDiscardShelfEntry}
                onPreviewShelfEntry={git.onPreviewShelfEntry}
              />
            ) : activeTool === "asciidoc" && asciidoc ? (
              <AsciiDocPanel
                canInsert={asciidoc.canInsert}
                onInsert={asciidoc.onInsert}
              />
            ) : activeTool === "utilities" && utilities ? (
              <UtilitiesPanel
                onOpen={utilities.onOpen}
                activeId={utilities.activeId}
                onNewArtifact={utilities.onNewArtifact}
                onOpenArtifacts={utilities.onOpenArtifacts}
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
                onDelete={branches.onDelete}
              />
            ) : activeTool === "suggestions" ? (
              <NotificationsPanel gitActionLog={gitActionLog ?? undefined} />
            ) : (
              <div className="panel-empty">{active.empty}</div>
            )}
          </div>
        </div>
      ) : null}

      {assistantMounted && assistant ? (
        <div className={showAssistant ? "tool-window" : "tool-window is-hidden"}>
          <ToolWindowHeader label={TOOL_DEFS.assistant.label} onHide={onHide} />
          <div className="tool-window-body">
            <AssistantPanel
              onOpenSettings={assistant.onOpenSettings}
              specsRepoInfo={assistant.specsRepoInfo}
              docsRoot={assistant.docsRoot}
              onFileWritten={assistant.onFileWritten}
              onFileMoved={assistant.onFileMoved}
              repoRoot={assistant.repoRoot}
              activeFilePath={assistant.activeFilePath}
              chatInsertRequest={chatInsertRequest ?? null}
              onChatInsertHandled={onChatInsertHandled}
              assistantSendRequest={assistantSendRequest ?? null}
              onAssistantSendHandled={onAssistantSendHandled}
              assistantDraftRequest={assistantDraftRequest ?? null}
              onAssistantDraftHandled={onAssistantDraftHandled}
            />
          </div>
        </div>
      ) : showAssistant ? (
        <div className="tool-window">
          <ToolWindowHeader label={TOOL_DEFS.assistant.label} onHide={onHide} />
          <div className="tool-window-body">
            <div className="panel-empty">{TOOL_DEFS.assistant.empty}</div>
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
