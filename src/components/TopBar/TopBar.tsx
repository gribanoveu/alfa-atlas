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

function pluralErrors(n: number): string {
  const mod10 = n % 10;
  const mod100 = n % 100;
  if (mod10 === 1 && mod100 !== 11) return "ошибка";
  if (mod10 >= 2 && mod10 <= 4 && (mod100 < 10 || mod100 >= 20)) return "ошибки";
  return "ошибок";
}

function pluralWarnings(n: number): string {
  const mod10 = n % 10;
  const mod100 = n % 100;
  if (mod10 === 1 && mod100 !== 11) return "предупреждение";
  if (mod10 >= 2 && mod10 <= 4 && (mod100 < 10 || mod100 >= 20)) return "предупреждения";
  return "предупреждений";
}

function TopBarStatusIcon({
  errorCount,
  warningCount,
}: {
  errorCount: number;
  warningCount: number;
}) {
  if (errorCount > 0) {
    return (
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <circle cx="8" cy="8" r="7" fill="currentColor" />
        <path
          d="M8 4v5"
          stroke="var(--bg-0)"
          strokeWidth="1.6"
          strokeLinecap="round"
        />
        <circle cx="8" cy="11.6" r="1" fill="var(--bg-0)" />
      </svg>
    );
  }
  if (warningCount > 0) {
    return (
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M8 1.5L15 14H1z" fill="currentColor" />
        <path
          d="M8 6.5v3"
          stroke="var(--bg-0)"
          strokeWidth="1.6"
          strokeLinecap="round"
        />
        <circle cx="8" cy="11.2" r="0.9" fill="var(--bg-0)" />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <circle cx="8" cy="8" r="7" fill="currentColor" />
      <path
        d="M5 8.2l2 2 4-4.4"
        stroke="var(--bg-0)"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
    </svg>
  );
}

type TopBarProps = {
  repoName?: string;
  branchName?: string;
  projectRoot: string | null;
  hasProject: boolean;
  gitBusy?: boolean;
  errorCount: number;
  warningCount: number;
  onOpenProblems: () => void;
  onOpenFolder: () => Promise<unknown>;
  onCloseProject: () => Promise<void>;
  onSave: () => Promise<unknown>;
  onPrefsChange?: (prefs: GeneralPrefs) => void;
  onToggleSidebar: () => void;
  onToggleRight: () => void;
  onToggleBottom: () => void;
  onToggleGit: () => void;
  onPull: () => void;
  onPush: () => void;
};

export function TopBar({
  repoName = "—",
  branchName = "—",
  projectRoot,
  hasProject,
  gitBusy = false,
  errorCount,
  warningCount,
  onOpenProblems,
  onOpenFolder,
  onCloseProject,
  onSave,
  onPrefsChange,
  onToggleSidebar,
  onToggleRight,
  onToggleBottom,
  onToggleGit,
  onPull,
  onPush,
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
        case "git.toggleCommit":
          if (hasProject) onToggleGit();
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
      onOpenFolder,
      onPull,
      onPush,
      onSave,
      onToggleBottom,
      onToggleGit,
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
        />
        <div className="topbar-spacer" />
        <div className="topbar-right">
          {hasProject ? (
            <button
              type="button"
              className={`topbar-status ${errorCount > 0 ? "has-errors" : warningCount > 0 ? "has-warnings" : "is-clean"}`}
              onClick={onOpenProblems}
              title={
                errorCount > 0
                  ? `${errorCount} ${pluralErrors(errorCount)} · ${warningCount} ${pluralWarnings(warningCount)}`
                  : warningCount > 0
                    ? `${warningCount} ${pluralWarnings(warningCount)}`
                    : "Нет проблем в индексе"
              }
              aria-label="Открыть панель проблем"
            >
              <TopBarStatusIcon
                errorCount={errorCount}
                warningCount={warningCount}
              />
              <span className="topbar-status-text">
                {errorCount > 0
                  ? `${errorCount} ${pluralErrors(errorCount)}`
                  : warningCount > 0
                    ? `${warningCount} ${pluralWarnings(warningCount)}`
                    : "OK"}
              </span>
            </button>
          ) : null}
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
