import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, CloudUpload, FolderOpen } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { appConfig } from "../../lib/appConfig";
import type { MenuActionId } from "../../lib/menuActions";
import type { GeneralPrefs } from "../../lib/prefs";
import type { ProbeResult } from "../../lib/git";
import type { SpellcheckConfig } from "../../lib/spellcheck";
import { SettingsDialog } from "../Settings/SettingsDialog";
import type { SectionId } from "../Settings/SettingsDialog";
import { CloneRepoModal } from "../Welcome/CloneRepoModal";
import { AboutModal } from "./AboutModal";
import { MenuBar } from "./MenuBar";
import { RecentProjectsDropdown } from "./RecentProjectsDropdown";
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
  canGoBack?: boolean;
  canGoForward?: boolean;
  hasUnpushedChanges?: boolean;
  onOpenPushConfirm?: () => void;
  onSelectProject?: (root: string) => void;
  onCloneProject?: (probe: ProbeResult) => Promise<void>;
  /** Bump this (e.g. `n => n + 1`) to open Settings on the "standards" tab. */
  openStandardsSettingsSignal?: number;
  /** Bump this (e.g. `n => n + 1`) to open Settings on the "embeddings" tab. */
  openEmbeddingsSettingsSignal?: number;
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
  canGoBack = false,
  canGoForward = false,
  hasUnpushedChanges = false,
  onOpenPushConfirm,
  onSelectProject,
  onCloneProject,
  openStandardsSettingsSignal,
  openEmbeddingsSettingsSignal,
}: TopBarProps) {
  const [aboutOpen, setAboutOpen] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsInitialSection, setSettingsInitialSection] = useState<SectionId | undefined>(undefined);
  const [recentDropdownOpen, setRecentDropdownOpen] = useState(false);
  const standardsSignalRef = useRef(openStandardsSettingsSignal);
  const embeddingsSignalRef = useRef(openEmbeddingsSettingsSignal);

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
    if (
      openEmbeddingsSettingsSignal === undefined ||
      openEmbeddingsSettingsSignal === embeddingsSignalRef.current
    ) {
      return;
    }
    embeddingsSignalRef.current = openEmbeddingsSettingsSignal;
    setSettingsInitialSection("embeddings");
    setSettingsOpen(true);
  }, [openEmbeddingsSettingsSignal]);

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
          void invoke("exit_app");
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
          {hasUnpushedChanges ? (
            <button
              type="button"
              className="unpushed-indicator-btn"
              title="Есть неотправленные изменения — отправить на сервер"
              aria-label="Есть неотправленные изменения — отправить на сервер"
              onClick={onOpenPushConfirm}
            >
              <CloudUpload size={16} />
            </button>
          ) : null}
          <div className="repo-chip-wrapper">
            <button
              type="button"
              className="repo-chip"
              onClick={() =>
                setRecentDropdownOpen(!recentDropdownOpen)
              }
              aria-expanded={recentDropdownOpen}
              aria-haspopup="menu"
            >
              <FolderOpen size={14} />
              <b>{repoName}</b>
            </button>
            {recentDropdownOpen ? (
              <RecentProjectsDropdown
                onSelect={(root) => {
                  setRecentDropdownOpen(false);
                  onSelectProject?.(root);
                }}
                onClose={() => setRecentDropdownOpen(false)}
              />
            ) : null}
          </div>
          <button
            type="button"
            className="branch-chip"
            disabled={!hasProject || gitBusy || branchBusy}
            onClick={onBranchChipClick}
            aria-expanded={branchesPanelOpen}
            aria-controls="branches-panel"
          >
            ⎇ {branchName}
          </button>
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
          onCloseProject={onCloseProject}
          onPrefsChange={onPrefsChange}
          onSpellcheckConfigChange={onSpellcheckConfigChange}
          initialSection={settingsInitialSection}
        />
      ) : null}
    </>
  );
}
