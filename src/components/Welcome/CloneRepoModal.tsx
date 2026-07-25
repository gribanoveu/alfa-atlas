import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import "./CloneRepoModal.css";

type CloneRepoModalProps = {
  onClose: () => void;
};

export function CloneRepoModal({ onClose }: CloneRepoModalProps) {
  const [url, setUrl] = useState("");
  const [destination, setDestination] = useState("");
  const [message, setMessage] = useState<string | null>(null);

  const pickDestination = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Папка для клонирования",
    });
    if (selected === null || Array.isArray(selected)) return;
    setDestination(selected);
  };

  const submit = () => {
    setMessage(
      "Клонирование репозитория будет подключено позже. Сейчас можно открыть уже существующую папку.",
    );
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
            type="url"
            placeholder="https://github.com/org/repo.git"
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            autoFocus
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
            />
            <button
              type="button"
              className="clone-modal-browse"
              onClick={() => void pickDestination()}
            >
              Обзор…
            </button>
          </div>
        </label>

        {message ? <div className="clone-modal-message">{message}</div> : null}

        <div className="clone-modal-actions">
          <button type="button" className="clone-modal-btn" onClick={onClose}>
            Отмена
          </button>
          <button
            type="button"
            className="clone-modal-btn primary"
            onClick={submit}
            disabled={!url.trim() || !destination.trim()}
          >
            Клонировать
          </button>
        </div>
      </div>
    </div>
  );
}
