import { openUrl } from "@tauri-apps/plugin-opener";
import { ChevronDown, ChevronLeft, ChevronRight, FolderOpen, GitBranch } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { exitApp } from "../../lib/app";
import { appConfig } from "../../lib/appConfig";
import type { MenuActionId } from "../../lib/menuActions";
import type { GeneralPrefs } from "../../lib/prefs";
import type { ProbeResult, SyncPillState } from "../../lib/git";
import type { SpellcheckConfig } from "../../lib/spellcheck";
import { SettingsDialog } from "../Settings/SettingsDialog";
import type { SectionId } from "../Settings/SettingsDialog";
import { CloneRepoModal } from "../Welcome/CloneRepoModal";
import { ToolCallLogModal } from "../ToolLog/ToolCallLogModal";
import { MemoryLogModal } from "../MemoryLog/MemoryLogModal";
import { PlansModal } from "../Plans/PlansModal";
import { AboutModal } from "./AboutModal";
import { MenuBar } from "./MenuBar";
import { RecentProjectsDropdown } from "./RecentProjectsDropdown";
import { SyncStatusPill } from "./SyncStatusPill";
import "./TopBar.css";

type TopBarProps = {
  repoName?: string;
  branchName?: string;
  projectRoot: string | null;
  hasProject: boolean;
  gitBusy?: boolean;
  branchesPanelOpen?: boolean;
  branchBusy?: boolean;
  onBranchChipClick?: () => void;
  onOpenFolder: () => Promise<unknown>;
  onCloseProject: () => Promise<void>;
  onSave: () => Promise<unknown>;
  onUndo?: () => void;
  onRedo?: () => void;
  hasActiveTab?: boolean;
  onPrefsChange?: (prefs: GeneralPrefs) => void;
  onSpellcheckConfigChange?: (config: SpellcheckConfig) => void;
  onToggleSidebar: () => void;
  onToggleRight: () => void;
  onToggleBottom: () => void;
  onToggleGit: () => void;
  onOpenBranches: () => void;
  onPull: () => void;
  onPush: () => void;
  onGoBack?: () => void;
  onGoForward?: () => void;
  onFindInDocs?: () => void;
  canGoBack?: boolean;
  canGoForward?: boolean;
  syncPillState: SyncPillState;
  onSyncPillClick: () => void;
  onSelectProject?: (root: string) => void;
  onCloneProject?: (probe: ProbeResult) => Promise<void>;
  /** Bump this (e.g. `n => n + 1`) to open Settings on the "standards" tab. */
  openStandardsSettingsSignal?: number;
  /** Bump this (e.g. `n => n + 1`) to open Settings on the "llm" tab. */
  openLlmSettingsSignal?: number;
};

export function TopBar({
  repoName = "—",
  branchName = "—",
  projectRoot,
  hasProject,
  gitBusy = false,
  branchesPanelOpen = false,
  branchBusy = false,
  onBranchChipClick,
  onOpenFolder,
  onCloseProject,
  onSave,
  onUndo,
  onRedo,
  hasActiveTab = false,
  onPrefsChange,
  onSpellcheckConfigChange,
  onToggleSidebar,
  onToggleRight,
  onToggleBottom,
  onToggleGit,
  onOpenBranches,
  onPull,
  onPush,
  onGoBack,
  onGoForward,
  onFindInDocs,
  canGoBack = false,
  canGoForward = false,
  syncPillState,
  onSyncPillClick,
  onSelectProject,
  onCloneProject,
  openStandardsSettingsSignal,
  openLlmSettingsSignal,
}: TopBarProps) {
  const [aboutOpen, setAboutOpen] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [toolLogOpen, setToolLogOpen] = useState(false);
  const [memoryLogOpen, setMemoryLogOpen] = useState(false);
  const [plansOpen, setPlansOpen] = useState(false);
  const [settingsInitialSection, setSettingsInitialSection] = useState<SectionId | undefined>(undefined);
  const [recentDropdownOpen, setRecentDropdownOpen] = useState(false);
  const repoChipRef = useRef<HTMLDivElement>(null);
  const standardsSignalRef = useRef(openStandardsSettingsSignal);
  const llmSignalRef = useRef(openLlmSettingsSignal);

  useEffect(() => {
    if (
      openStandardsSettingsSignal === undefined ||
      openStandardsSettingsSignal === standardsSignalRef.current
    ) {
      return;
    }
    standardsSignalRef.current = openStandardsSettingsSignal;
    setSettingsInitialSection("standards");
    setSettingsOpen(true);
  }, [openStandardsSettingsSignal]);

  useEffect(() => {
    if (openLlmSettingsSignal === undefined || openLlmSettingsSignal === llmSignalRef.current) {
      return;
    }
    llmSignalRef.current = openLlmSettingsSignal;
    setSettingsInitialSection("llm");
    setSettingsOpen(true);
  }, [openLlmSettingsSignal]);

  const onAction = useCallback(
    (action: MenuActionId) => {
      switch (action) {
        case "file.openFolder":
          void onOpenFolder();
          break;
        case "file.cloneRepo":
        case "git.cloneRepo":
          setCloneOpen(true);
          break;
        case "file.save":
          if (hasProject) void onSave();
          break;
        case "file.closeProject":
          if (hasProject) void onCloseProject();
          break;
        case "file.exit":
          void exitApp();
          break;
        case "edit.undo":
          onUndo?.();
          break;
        case "edit.redo":
          onRedo?.();
          break;
        case "view.toggleSidebar":
          onToggleSidebar();
          break;
        case "view.toggleRight":
          onToggleRight();
          break;
        case "view.toggleBottom":
          onToggleBottom();
          break;
        case "nav.goBack":
          onGoBack?.();
          break;
        case "nav.goForward":
          onGoForward?.();
          break;
        case "nav.findInDocs":
          if (hasProject) onFindInDocs?.();
          break;
        case "git.toggleCommit":
          if (hasProject) onToggleGit();
          break;
        case "git.createBranch":
          if (hasProject) onOpenBranches();
          break;
        case "git.pull":
          if (hasProject) onPull();
          break;
        case "git.push":
          if (hasProject) onPush();
          break;
        case "tools.settings":
          setSettingsInitialSection(undefined);
          setSettingsOpen(true);
          break;
        case "tools.toolLog":
          setToolLogOpen(true);
          break;
        case "tools.memoryLog":
          setMemoryLogOpen(true);
          break;
        case "tools.plans":
          setPlansOpen(true);
          break;
        case "help.about":
          setAboutOpen(true);
          break;
        case "help.docs":
          void openUrl(appConfig.documentationUrl);
          break;
        case "help.feedback":
          void openUrl(appConfig.feedbackUrl);
          break;
        case "help.updates":
          void openUrl(appConfig.updatesUrl);
          break;
      }
    },
    [
      hasProject,
      onCloseProject,
      onGoBack,
      onGoForward,
      onFindInDocs,
      onOpenFolder,
      onPull,
      onPush,
      onSave,
      onUndo,
      onRedo,
      onToggleBottom,
      onToggleGit,
      onOpenBranches,
      onToggleRight,
      onToggleSidebar,
    ],
  );

  return (
    <>
      <header className="topbar">
        <MenuBar
          onAction={onAction}
          hasProject={hasProject}
          gitBusy={gitBusy}
          hasActiveTab={hasActiveTab}
        />
        <div className="topbar-spacer" />
        <div className="topbar-right">
          <div className="topbar-nav-buttons">
            <button
              type="button"
              className="nav-btn"
              disabled={!canGoBack}
              title="Назад"
              onClick={onGoBack}
            >
              <ChevronLeft size={16} />
            </button>
            <button
              type="button"
              className="nav-btn"
              disabled={!canGoForward}
              title="Вперёд"
              onClick={onGoForward}
            >
              <ChevronRight size={16} />
            </button>
          </div>
          {hasProject ? (
            <div className="topbar-context">
              <SyncStatusPill state={syncPillState} onClick={onSyncPillClick} />
              <span className="topbar-context-divider" aria-hidden />
              <div className="repo-chip-wrapper" ref={repoChipRef}>
                <button
                  type="button"
                  className="repo-chip"
                  onClick={() => setRecentDropdownOpen((open) => !open)}
                  aria-expanded={recentDropdownOpen}
                  aria-haspopup="menu"
                  title="Недавние проекты"
                >
                  <FolderOpen size={13} aria-hidden />
                  <span className="repo-chip-name">{repoName}</span>
                  <ChevronDown size={12} className="repo-chip-chevron" aria-hidden />
                </button>
                {recentDropdownOpen ? (
                  <RecentProjectsDropdown
                    anchorRef={repoChipRef}
                    onSelect={(root) => {
                      setRecentDropdownOpen(false);
                      onSelectProject?.(root);
                    }}
                    onClose={() => setRecentDropdownOpen(false)}
                  />
                ) : null}
              </div>
              <span className="topbar-context-divider" aria-hidden />
              <button
                type="button"
                className={`branch-chip${branchesPanelOpen ? " is-open" : ""}`}
                disabled={!hasProject || gitBusy || branchBusy}
                onClick={onBranchChipClick}
                aria-expanded={branchesPanelOpen}
                aria-controls="branches-panel"
                title="Ветки"
              >
                <GitBranch size={13} aria-hidden />
                <span className="branch-chip-name">{branchName}</span>
              </button>
            </div>
          ) : null}
        </div>
      </header>

      {aboutOpen ? <AboutModal onClose={() => setAboutOpen(false)} /> : null}
      {cloneOpen ? (
        <CloneRepoModal
          onClose={() => setCloneOpen(false)}
          onOpenSettings={() => {
            setSettingsInitialSection("credentials");
            setSettingsOpen(true);
          }}
          onOpened={async (probe) => {
            setCloneOpen(false);
            if (onCloneProject) {
              await onCloneProject(probe);
            }
          }}
        />
      ) : null}
      {settingsOpen ? (
        <SettingsDialog
          projectRoot={projectRoot}
          onClose={() => {
            setSettingsOpen(false);
            setSettingsInitialSection(undefined);
          }}
          onPrefsChange={onPrefsChange}
          onSpellcheckConfigChange={onSpellcheckConfigChange}
          initialSection={settingsInitialSection}
        />
      ) : null}
      {toolLogOpen ? <ToolCallLogModal projectRoot={projectRoot} onClose={() => setToolLogOpen(false)} /> : null}
      {memoryLogOpen ? (
        <MemoryLogModal projectRoot={projectRoot} onClose={() => setMemoryLogOpen(false)} />
      ) : null}
      {plansOpen ? (
        <PlansModal
          onClose={() => setPlansOpen(false)}
          onStartPlan={(planId) => {
            window.dispatchEvent(new CustomEvent("atlas-start-plan", { detail: { planId } }));
          }}
          onOpenInEditor={(planId) => {
            window.dispatchEvent(new CustomEvent("atlas-open-plan", { detail: { planId } }));
            setPlansOpen(false);
          }}
        />
      ) : null}
    </>
  );
}
