import { useState, useCallback } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { SshKeyConfig, SshKeySource } from "../../lib/git";
import "../Welcome/CloneRepoModal.css";
import "./AddSshKeyModal.css";

type AddSshKeyModalProps = {
  initial: SshKeyConfig | null;
  onSave: (config: SshKeyConfig) => void;
  onClose: () => void;
};

export function AddSshKeyModal({
  initial,
  onSave,
  onClose,
}: AddSshKeyModalProps) {
  const [name, setName] = useState(initial?.name ?? "");
  const [host, setHost] = useState(initial?.host ?? "");
  const [sourceKind, setSourceKind] = useState<"keyContent" | "keyFile">(
    initial?.source.kind ?? "keyContent",
  );
  const [keyContent, setKeyContent] = useState(
    initial?.source.kind === "keyContent" ? initial.source.privateKey : "",
  );
  const [keyPath, setKeyPath] = useState(
    initial?.source.kind === "keyFile" ? initial.source.path : "",
  );
  const [passphrase, setPassphrase] = useState(
    initial?.passphrase ?? "",
  );
  const [error, setError] = useState<string | null>(null);

  const pickKeyFile = useCallback(async () => {
    try {
      const selected = await open({
        multiple: false,
        title: "Выберите файл SSH ключа",
      });
      if (selected === null || Array.isArray(selected)) return;
      setKeyPath(selected);
    } catch {
      // dialog cancelled
    }
  }, []);

  const handleSubmit = () => {
    setError(null);
    const trimmedName = name.trim();
    if (!trimmedName) {
      setError("Имя ключа обязательно");
      return;
    }

    const source: SshKeySource =
      sourceKind === "keyContent"
        ? { kind: "keyContent", privateKey: keyContent }
        : { kind: "keyFile", path: keyPath };

    if (sourceKind === "keyContent" && !keyContent.trim()) {
      setError("Содержимое ключа обязательно");
      return;
    }
    if (sourceKind === "keyFile" && !keyPath.trim()) {
      setError("Путь к файлу ключа обязателен");
      return;
    }

    onSave({
      name: trimmedName,
      host: host.trim() || undefined,
      source,
      passphrase: passphrase || undefined,
    });
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
        aria-labelledby="ssh-key-modal-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="ssh-key-modal-title">
          {initial ? "Изменить SSH ключ" : "Добавить SSH ключ"}
        </div>

        <label className="clone-modal-field">
          <span className="clone-modal-label">Название</span>
          <input
            className="clone-modal-input"
            type="text"
            placeholder="Например: Bitbucket рабочий"
            value={name}
            onChange={(event) => setName(event.target.value)}
            autoFocus
          />
        </label>

        <label className="clone-modal-field">
          <span className="clone-modal-label">
            Хост (необязательно)
          </span>
          <input
            className="clone-modal-input"
            type="text"
            placeholder="bitbucket.company.com"
            value={host}
            onChange={(event) => setHost(event.target.value)}
          />
          <span className="clone-modal-hint">
            Если указан, ключ будет применяться только для репозиториев, URL
            которых содержит этот хост.
          </span>
        </label>

        <div className="clone-modal-field">
          <span className="clone-modal-label">Источник ключа</span>
          <div className="ssh-key-source-toggle">
            <button
              type="button"
              className={`ssh-key-source-btn${sourceKind === "keyContent" ? " active" : ""}`}
              onClick={() => setSourceKind("keyContent")}
            >
              Содержимое
            </button>
            <button
              type="button"
              className={`ssh-key-source-btn${sourceKind === "keyFile" ? " active" : ""}`}
              onClick={() => setSourceKind("keyFile")}
            >
              Файл
            </button>
          </div>
        </div>

        {sourceKind === "keyContent" ? (
          <label className="clone-modal-field">
            <span className="clone-modal-label">Приватный ключ</span>
            <textarea
              className="clone-modal-input ssh-key-textarea"
              placeholder="-----BEGIN OPENSSH PRIVATE KEY-----&#10;..."
              value={keyContent}
              onChange={(event) => setKeyContent(event.target.value)}
              rows={8}
            />
          </label>
        ) : (
          <label className="clone-modal-field">
            <span className="clone-modal-label">Файл ключа</span>
            <div className="clone-modal-path-row">
              <input
                className="clone-modal-input"
                type="text"
                placeholder="~/.ssh/id_ed25519"
                value={keyPath}
                onChange={(event) => setKeyPath(event.target.value)}
              />
              <button
                type="button"
                className="clone-modal-browse"
                onClick={() => void pickKeyFile()}
              >
                Обзор…
              </button>
            </div>
          </label>
        )}

        <label className="clone-modal-field">
          <span className="clone-modal-label">
            Парольная фраза (необязательно)
          </span>
          <input
            className="clone-modal-input"
            type="password"
            placeholder="Оставьте пустым, если ключ без пароля"
            value={passphrase}
            onChange={(event) => setPassphrase(event.target.value)}
          />
        </label>

        {error ? (
          <div className="clone-modal-message" style={{ color: "var(--danger)" }}>
            {error}
          </div>
        ) : null}

        <div className="clone-modal-actions">
          <button type="button" className="clone-modal-btn" onClick={onClose}>
            Отмена
          </button>
          <button
            type="button"
            className="clone-modal-btn primary"
            onClick={handleSubmit}
          >
            {initial ? "Сохранить" : "Добавить"}
          </button>
        </div>
      </div>
    </div>
  );
}
