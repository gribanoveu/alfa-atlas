import { FolderGit2, FolderOpen } from "lucide-react";
import { useState } from "react";
import { CloneRepoModal } from "./CloneRepoModal";
import "./Welcome.css";

type WelcomeProps = {
  onOpenFolder: () => Promise<unknown>;
  error?: string | null;
};

export function Welcome({ onOpenFolder, error }: WelcomeProps) {
  const [busy, setBusy] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);

  const handleOpenFolder = async () => {
    setBusy(true);
    setLocalError(null);
    try {
      await onOpenFolder();
    } catch (e) {
      setLocalError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const displayError = localError ?? error;

  return (
    <section className="welcome">
      <div className="welcome-inner">
        <header className="welcome-brand">
          <div className="welcome-brand-row">
            <span className="welcome-dot" />
            <h1 className="welcome-title">docflow</h1>
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
          <div className="welcome-recent-empty">Пока нет недавних проектов</div>
        </section>
      </div>

      {cloneOpen ? (
        <CloneRepoModal onClose={() => setCloneOpen(false)} />
      ) : null}
    </section>
  );
}
