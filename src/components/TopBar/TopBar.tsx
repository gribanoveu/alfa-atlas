import { openUrl } from "@tauri-apps/plugin-opener";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useState } from "react";
import { appConfig } from "../../lib/appConfig";
import type { MenuActionId } from "../../lib/menuActions";
import type { GeneralPrefs } from "../../lib/prefs";
import { SettingsDialog } from "../Settings/SettingsDialog";
import { CloneRepoModal } from "../Welcome/CloneRepoModal";
import { AboutModal } from "./AboutModal";
import { MenuBar } from "./MenuBar";
import "./TopBar.css";

type TopBarProps = {
  repoName?: string;
  branchName?: string;
  projectRoot: string | null;
  hasProject: boolean;
  onOpenFolder: () => Promise<unknown>;
  onCloseProject: () => Promise<void>;
  onSave: () => Promise<unknown>;
  onPrefsChange?: (prefs: GeneralPrefs) => void;
  onToggleSidebar: () => void;
  onToggleRight: () => void;
  onToggleBottom: () => void;
};

export function TopBar({
  repoName = "—",
  branchName = "—",
  projectRoot,
  hasProject,
  onOpenFolder,
  onCloseProject,
  onSave,
  onPrefsChange,
  onToggleSidebar,
  onToggleRight,
  onToggleBottom,
}: TopBarProps) {
  const [aboutOpen, setAboutOpen] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);

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
      onOpenFolder,
      onSave,
      onToggleBottom,
      onToggleRight,
      onToggleSidebar,
    ],
  );

  return (
    <>
      <header className="topbar">
        <MenuBar onAction={onAction} />
        <div className="topbar-spacer" />
        <div className="topbar-right">
          <div className="repo-chip">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M9 2v20M15 2v6a3 3 0 0 1-3 3H6M9 8h.01" />
            </svg>
            <b>{repoName}</b>
          </div>
          <div className="branch-chip">⎇ {branchName}</div>
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
