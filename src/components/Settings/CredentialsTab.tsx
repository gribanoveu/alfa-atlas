import { useState } from "react";
import type { SshKeyConfig } from "../../lib/git";
import { useGitCredentials } from "../../hooks/useGitCredentials";
import { AddSshKeyModal } from "./AddSshKeyModal";
import "../Welcome/CloneRepoModal.css";
import "./CredentialsTab.css";

export function CredentialsTab() {
  const {
    credentials,
    keyStatus,
    error,
    busy,
    keyGenBusy,
    copyFeedback,
    toggleTrustAll,
    deleteKey,
    saveKey,
    generateKey,
    importKey,
    copyPublicKey,
  } = useGitCredentials();

  // Which dialog is open, and which row it is editing — presentation only.
  const [showAddModal, setShowAddModal] = useState(false);
  const [editIndex, setEditIndex] = useState<number | null>(null);
  const [showRegenerateConfirm, setShowRegenerateConfirm] = useState(false);

  const handleSave = (config: SshKeyConfig) => {
    saveKey(config, editIndex);
    setShowAddModal(false);
    setEditIndex(null);
  };

  const openAdd = () => {
    setEditIndex(null);
    setShowAddModal(true);
  };

  const openEdit = (index: number) => {
    setEditIndex(index);
    setShowAddModal(true);
  };

  if (!credentials || !keyStatus) {
    return (
      <div className="credentials-tab">
        {error ? (
          <div className="settings-error">{error}</div>
        ) : (
          <p>Загрузка...</p>
        )}
      </div>
    );
  }

  const editKey =
    editIndex !== null ? credentials.sshKeys[editIndex] ?? null : null;

  return (
    <div className="settings-sections credentials-tab">
      <div className="settings-card">
      {/* Section 1: App Key */}
      <div className="settings-section-title">Ключ приложения</div>
      <p className="settings-hint settings-hint-compact">
        Atlas использует Ed25519 SSH ключ для авторизации в Git. 
        Закрытый ключ хранится зашифрованным. Для авторизации приложения в Git, 
        необходимо добавить его в раздел SSH и GPG ключей в вашем аккаунте Bitbucket, GitHub, GitLab и т.д.
      </p>

      {!keyStatus.exists ? (
        <div className="credentials-app-key-none">
          <p className="settings-hint settings-hint-compact">
            SSH ключ не настроен. Сгенерируйте новый ключ или импортируйте
            существующий.
          </p>
          <div className="settings-actions">
            <button
              type="button"
              className="settings-btn primary"
              disabled={keyGenBusy}
              onClick={() => void generateKey()}
            >
              {keyGenBusy ? "Генерация..." : "Сгенерировать ключ"}
            </button>
            <button
              type="button"
              className="settings-btn"
              disabled={keyGenBusy}
              onClick={() => void importKey()}
            >
              Импортировать из файла...
            </button>
          </div>
        </div>
      ) : (
        <div className="credentials-app-key">
          <div className="credentials-app-key-header">
            <span className="credentials-app-key-badge">
              {keyStatus.isImported ? "Импортированный ключ" : "Сгенерированный ключ"}
            </span>
            {keyStatus.privateKeyAvailable ? (
              <span className="credentials-app-key-status ok">
                Активен
              </span>
            ) : (
              <span className="credentials-app-key-status error">
                Не удалось расшифровать закрытый ключ
              </span>
            )}
          </div>
          <label className="clone-modal-field">
            <span className="clone-modal-label">Открытый ключ</span>
            <textarea
              className="clone-modal-input ssh-key-textarea"
              value={keyStatus.publicKey}
              readOnly
              rows={3}
            />
          </label>
          <div className="settings-actions">
            <button
              type="button"
              className="settings-btn primary"
              onClick={copyPublicKey}
            >
              {copyFeedback ? "Скопировано!" : "Копировать"}
            </button>
            <button
              type="button"
              className="settings-btn"
              disabled={keyGenBusy}
              onClick={() => setShowRegenerateConfirm(true)}
            >
              {keyGenBusy ? "Генерация..." : "Перегенерировать"}
            </button>
            <button
              type="button"
              className="settings-btn"
              disabled={keyGenBusy}
              onClick={() => void importKey()}
            >
              Импортировать из файла...
            </button>
          </div>
        </div>
      )}

      </div>

      <div className="settings-card">
      {/* Section: SSH host key verification */}
      <div className="settings-section-title">Проверка SSH хостов</div>
      <label className="settings-check">
        <input
          type="checkbox"
          checked={credentials.trustAllSshHostKeys}
          onChange={toggleTrustAll}
          disabled={busy}
        />
        <span>Принимать все SSH сертификаты (Trust-On-First-Use)</span>
      </label>
      <p className="settings-hint">
        Когда включено, приложение принимает любой SSH ключ хоста при первом
        подключении. Когда выключено, используется стандартная проверка через{" "}
        <code>~/.ssh/known_hosts</code>. Отключите для максимальной
        безопасности, если у вас настроен файл known_hosts.
      </p>

      </div>

      <div className="settings-card">
      <div className="settings-section-title">
        Дополнительные SSH ключи
      </div>
      <p className="settings-hint settings-hint-compact">
        Дополнительные SSH ключи для специфичных хостов. Приоритет: ключ приложения,
        затем SSH-агент, затем ключи из этого списка.
      </p>

      {credentials.sshKeys.length === 0 ? (
        <p className="settings-hint settings-hint-compact">Нет сохранённых SSH ключей.</p>
      ) : (
        <div className="credentials-list">
          {credentials.sshKeys.map((key, index) => (
            <div key={index} className="credentials-item">
              <div className="credentials-item-info">
                <span className="credentials-item-name">{key.name}</span>
                {key.host ? (
                  <span className="credentials-item-host">{key.host}</span>
                ) : null}
                <span className="credentials-item-type">
                  {key.source.kind === "keyContent"
                    ? "Содержимое ключа"
                    : `Файл: ${key.source.path.slice(-30)}`}
                </span>
              </div>
              <div className="credentials-item-actions">
                <button
                  type="button"
                  className="settings-link-btn"
                  disabled={busy}
                  onClick={() => openEdit(index)}
                >
                  Изменить
                </button>
                <button
                  type="button"
                  className="settings-link-btn danger"
                  disabled={busy}
                  onClick={() => deleteKey(index)}
                >
                  Удалить
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      <div className="settings-actions">
        <button
          type="button"
          className="settings-btn primary"
          disabled={busy}
          onClick={openAdd}
        >
          Добавить SSH ключ
        </button>
      </div>

      </div>

      {showAddModal ? (
        <AddSshKeyModal
          initial={editKey}
          onSave={handleSave}
          onClose={() => {
            setShowAddModal(false);
            setEditIndex(null);
          }}
        />
      ) : null}

      {showRegenerateConfirm ? (
        <div
          className="clone-modal-backdrop"
          role="presentation"
          onClick={() => setShowRegenerateConfirm(false)}
        >
          <div
            className="clone-modal"
            role="dialog"
            aria-modal="true"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="clone-modal-title">
              Перегенерировать ключ приложения?
            </div>
            <div className="clone-modal-message">
              Текущий SSH ключ приложения будет заменён новым. Старый открытый
              ключ станет недействительным — доступ ко всем Git-репозиториям,
              где он был добавлен, пропадёт. После генерации потребуется
              заново добавить новый открытый ключ в настройки Git-провайдера.
            </div>
            <div className="clone-modal-actions">
              <button
                type="button"
                className="clone-modal-btn"
                onClick={() => setShowRegenerateConfirm(false)}
              >
                Отмена
              </button>
              <button
                type="button"
                className="clone-modal-btn primary danger"
                onClick={() => {
                  setShowRegenerateConfirm(false);
                  void generateKey();
                }}
              >
                Перегенерировать
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
