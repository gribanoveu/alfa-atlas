import { FolderGit2, FolderOpen, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import {
  listRecentProjects,
  removeRecentProject,
  type RecentProject,
} from "../../lib/project";
import { toMessage } from "../../lib/errors";
import type { ProbeResult } from "../../lib/git";
import { CloneRepoModal } from "./CloneRepoModal";
import "./Welcome.css";

type WelcomeProps = {
  onOpenFolder: () => Promise<unknown>;
  onOpenRecent: (root: string) => Promise<unknown>;
  onCloneProject?: (probe: ProbeResult) => Promise<unknown>;
  onOpenSettings?: () => void;
  error?: string | null;
};

export function Welcome({ onOpenFolder, onOpenRecent, onCloneProject, onOpenSettings, error }: WelcomeProps) {
  const [busy, setBusy] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [recent, setRecent] = useState<RecentProject[]>([]);

  const reloadRecent = useCallback(async () => {
    try {
      const items = await listRecentProjects();
      setRecent(items);
    } catch {
      setRecent([]);
    }
  }, []);

  useEffect(() => {
    void reloadRecent();
  }, [reloadRecent]);

  const handleOpenFolder = async () => {
    setBusy(true);
    setLocalError(null);
    try {
      await onOpenFolder();
    } catch (e) {
      setLocalError(toMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const handleOpenRecent = async (root: string) => {
    setBusy(true);
    setLocalError(null);
    try {
      await onOpenRecent(root);
    } catch (e) {
      setLocalError(toMessage(e));
      await reloadRecent();
    } finally {
      setBusy(false);
    }
  };

  const handleRemoveRecent = async (root: string) => {
    try {
      await removeRecentProject(root);
      await reloadRecent();
    } catch (e) {
      setLocalError(toMessage(e));
    }
  };

  const displayError = localError ?? error;

  return (
    <section className="welcome">
      <div className="welcome-inner">
        <header className="welcome-brand">
          <div className="welcome-brand-row">
            <span className="welcome-dot" />
            <h1 className="welcome-title">Alfa Atlas</h1>
          </div>
          <p className="welcome-subtitle">
            Добро пожаловать в редактор документации.
            Откройте папку проекта или склонируйте git-репозиторий,
            чтобы начать работу.
          </p>
        </header>

        <section className="welcome-section">
          <h2 className="welcome-section-title">Начало работы</h2>
          <div className="welcome-actions">
            <button
              type="button"
              className="welcome-action"
              disabled={busy}
              onClick={() => void handleOpenFolder()}
            >
              <span className="welcome-action-label">
                <FolderOpen className="welcome-action-icon" size={16} aria-hidden />
                Открыть папку…
              </span>
              <span className="welcome-action-hint">
                Выбрать локальный каталог с документацией
              </span>
            </button>
            <button
              type="button"
              className="welcome-action"
              onClick={() => setCloneOpen(true)}
            >
              <span className="welcome-action-label">
                <FolderGit2 className="welcome-action-icon" size={16} aria-hidden />
                Клонировать репозиторий…
              </span>
              <span className="welcome-action-hint">
                Склонировать git-репозиторий и открыть его
              </span>
            </button>
          </div>
          {displayError ? (
            <div className="welcome-error">{displayError}</div>
          ) : null}
        </section>

        <section className="welcome-section">
          <h2 className="welcome-section-title">Недавние</h2>
          {recent.length === 0 ? (
            <div className="welcome-recent-empty">Пока нет недавних проектов</div>
          ) : (
            <ul className="welcome-recent-list">
              {recent.map((item) => (
                <li key={item.root} className="welcome-recent-item">
                  <button
                    type="button"
                    className="welcome-recent-open"
                    disabled={busy}
                    onClick={() => void handleOpenRecent(item.root)}
                  >
                    <span className="welcome-recent-name">{item.name}</span>
                    <span className="welcome-recent-path">{item.root}</span>
                  </button>
                  <button
                    type="button"
                    className="welcome-recent-remove"
                    aria-label={`Убрать «${item.name}» из недавних`}
                    disabled={busy}
                    onClick={() => void handleRemoveRecent(item.root)}
                  >
                    <X size={14} aria-hidden />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>

      {cloneOpen ? (
        <CloneRepoModal
          onClose={() => setCloneOpen(false)}
          onOpenSettings={onOpenSettings}
          onOpened={async (project) => {
            setCloneOpen(false);
            if (onCloneProject) {
              await onCloneProject(project);
            }
          }}
        />
      ) : null}
    </section>
  );
}
