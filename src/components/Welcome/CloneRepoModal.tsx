import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
import { gitClone } from "../../lib/git";
import type { ProbeResult } from "../../lib/git";
import { checkPathExists } from "../../lib/project";
import { getGeneralPrefs, setGeneralPrefs } from "../../lib/prefs";
import "./CloneRepoModal.css";

function getRepoName(url: string): string | null {
  const trimmed = url.trim();
  if (!trimmed) return null;
  const withoutGit = trimmed.endsWith(".git") ? trimmed.slice(0, -4) : trimmed;
  const last = withoutGit.split("/").pop() ?? "";
  if (!last) return null;
  const colonIdx = last.lastIndexOf(":");
  return colonIdx >= 0 ? last.slice(colonIdx + 1) : last;
}

type CloneRepoModalProps = {
  onClose: () => void;
  onOpened?: (probe: ProbeResult) => void;
  onOpenSettings?: () => void;
};

export function CloneRepoModal({
  onClose,
  onOpened,
  onOpenSettings,
}: CloneRepoModalProps) {
  const [url, setUrl] = useState("");
  const [baseDir, setBaseDir] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [cloning, setCloning] = useState(false);
  const [needsAuth, setNeedsAuth] = useState(false);
  const [conflict, setConflict] = useState(false);

  const repoName = useMemo(() => getRepoName(url), [url]);

  const destination = repoName
    ? `${baseDir}/${repoName}`
    : baseDir;

  const handleDestinationChange = (value: string) => {
    const lastSlash = value.lastIndexOf("/");
    setBaseDir(lastSlash > 0 ? value.slice(0, lastSlash) : value);
  };

  useEffect(() => {
    getGeneralPrefs()
      .then((prefs) => {
        if (prefs.lastCloneDir) setBaseDir(prefs.lastCloneDir);
      })
      .catch(() => {});
  }, []);

  const pickDestination = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Папка для клонирования",
    });
    if (selected === null || Array.isArray(selected)) return;
    setBaseDir(selected);
    try {
      const current = await getGeneralPrefs();
      await setGeneralPrefs({ ...current, lastCloneDir: selected });
    } catch {
      // ignore persistence failure — selection still applies to this session
    }
  };

  useEffect(() => {
    if (!destination) {
      setConflict(false);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      checkPathExists(destination).then((result) => {
        if (!cancelled) setConflict(result.exists);
      });
    }, 400);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [destination]);

  const submit = async () => {
    setMessage(null);
    setNeedsAuth(false);
    setCloning(true);
    try {
      setMessage("Клонирование...");
      const project = await gitClone(url.trim(), destination.trim());
      setCloning(false);
      onOpened?.(project);
    } catch (e) {
      setCloning(false);
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.startsWith("no_ssh_credentials:")) {
        setNeedsAuth(true);
        setMessage(
          "Аутентификация не настроена. Добавьте SSH ключ в настройках, чтобы продолжить.",
        );
      } else {
        setMessage(msg);
      }
    }
  };

  const handleOpenSettings = () => {
    onClose();
    onOpenSettings?.();
  };

  const canSubmit =
    !url.trim() || !baseDir.trim() || !repoName || cloning || conflict;

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onClick={onClose}
    >
      <div
        className="clone-modal"
        role="dialog"
        aria-labelledby="clone-modal-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="clone-modal-title">
          Клонировать репозиторий
        </div>

        <label className="clone-modal-field">
          <span className="clone-modal-label">URL репозитория</span>
          <input
            className="clone-modal-input"
            type="text"
            placeholder="git@bitbucket.company.com:project/repo.git"
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            autoFocus
            disabled={cloning}
          />
        </label>

        <label className="clone-modal-field">
          <span className="clone-modal-label">Папка назначения</span>
          <div className="clone-modal-path-row">
            <input
              className="clone-modal-input"
              type="text"
              placeholder="Выберите папку…"
              value={destination}
              onChange={(event) => handleDestinationChange(event.target.value)}
              disabled={cloning}
            />
            <button
              type="button"
              className="clone-modal-browse"
              onClick={() => void pickDestination()}
              disabled={cloning}
            >
              Обзор…
            </button>
          </div>
          {conflict ? (
            <div className="clone-modal-hint" style={{ color: "var(--danger)" }}>
              Папка уже существует. Выберите другое расположение.
            </div>
          ) : null}
        </label>

        {message ? (
          <div
            className={`clone-modal-message${cloning ? " is-busy" : ""}`}
          >
            {message}
          </div>
        ) : null}

        {needsAuth && onOpenSettings ? (
          <div className="clone-modal-actions">
            <button
              type="button"
              className="clone-modal-btn primary"
              onClick={handleOpenSettings}
            >
              Открыть настройки
            </button>
          </div>
        ) : null}

        <div className="clone-modal-actions">
          <button
            type="button"
            className="clone-modal-btn"
            onClick={onClose}
            disabled={cloning}
          >
            Отмена
          </button>
          <button
            type="button"
            className="clone-modal-btn primary"
            onClick={() => void submit()}
            disabled={canSubmit}
          >
            {cloning ? "Клонирование…" : "Клонировать"}
          </button>
        </div>
      </div>
    </div>
  );
}