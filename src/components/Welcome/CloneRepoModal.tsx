import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { gitClone } from "../../lib/git";
import type { ProbeResult } from "../../lib/git";
import "./CloneRepoModal.css";

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
  const [destination, setDestination] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [cloning, setCloning] = useState(false);
  const [needsAuth, setNeedsAuth] = useState(false);

  const pickDestination = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Папка для клонирования",
    });
    if (selected === null || Array.isArray(selected)) return;
    setDestination(selected);
  };

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
              onChange={(event) => setDestination(event.target.value)}
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
            disabled={!url.trim() || !destination.trim() || cloning}
          >
            {cloning ? "Клонирование…" : "Клонировать"}
          </button>
        </div>
      </div>
    </div>
  );
}
