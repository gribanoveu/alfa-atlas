import { openUrl } from "@tauri-apps/plugin-opener";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { useCallback, useState } from "react";
import { appConfig } from "../../lib/appConfig";
import type { MenuActionId } from "../../lib/menuActions";
import type { GeneralPrefs } from "../../lib/prefs";
import { SettingsDialog } from "../Settings/SettingsDialog";
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
  onPrefsChange?: (prefs: GeneralPrefs) => void;
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
  onSelectProject?: (root: string) => void;
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
  onPrefsChange,
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
  onSelectProject,
}: TopBarProps) {
  const [aboutOpen, setAboutOpen] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [recentDropdownOpen, setRecentDropdownOpen] = useState(false);

  const onAction = useCallback(
    (action: MenuActionId) => {
      switch (action) {
        case "file.openFolder":
          void onOpenFolder();
          break;
        case "file.cloneRepo":
          setCloneOpen(true);
          break;
        case "file.save":
          if (hasProject) void onSave();
          break;
        case "file.closeProject":
          if (hasProject) void onCloseProject();
          break;
        case "file.exit":
          void getCurrentWindow().close();
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
          canGoBack={canGoBack}
          canGoForward={canGoForward}
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
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M9 2v20M15 2v6a3 3 0 0 1-3 3H6M9 8h.01" />
              </svg>
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
      {cloneOpen ? <CloneRepoModal onClose={() => setCloneOpen(false)} /> : null}
      {settingsOpen ? (
        <SettingsDialog
          projectRoot={projectRoot}
          onClose={() => setSettingsOpen(false)}
          onCloseProject={onCloseProject}
          onPrefsChange={onPrefsChange}
        />
      ) : null}
    </>
  );
}
