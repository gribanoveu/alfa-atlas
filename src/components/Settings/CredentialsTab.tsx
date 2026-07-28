import { useState, useEffect, useCallback } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  gitGetCredentials,
  gitSaveCredentials,
  gitGetKeyStatus,
  gitGenerateKey,
  gitImportKey,
  type GitCredentials,
  type SshKeyConfig,
  type AppKeyStatus,
} from "../../lib/git";
import { AddSshKeyModal } from "./AddSshKeyModal";
import "../Welcome/CloneRepoModal.css";
import "./CredentialsTab.css";

export function CredentialsTab() {
  const [credentials, setCredentials] = useState<GitCredentials | null>(null);
  const [keyStatus, setKeyStatus] = useState<AppKeyStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showAddModal, setShowAddModal] = useState(false);
  const [editIndex, setEditIndex] = useState<number | null>(null);
  const [keyGenBusy, setKeyGenBusy] = useState(false);
  const [copyFeedback, setCopyFeedback] = useState(false);
  const [showRegenerateConfirm, setShowRegenerateConfirm] = useState(false);

  const handleToggleTrustAll = () => {
    if (!credentials) return;
    void persist({ ...credentials, trustAllSshHostKeys: !credentials.trustAllSshHostKeys });
  };

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [creds, status] = await Promise.all([
          gitGetCredentials(),
          gitGetKeyStatus(),
        ]);
        if (!cancelled) {
          setCredentials(creds);
          setKeyStatus(status);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) {
          setError(e instanceof Error ? e.message : String(e));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const persist = async (creds: GitCredentials) => {
    setCredentials(creds);
    setBusy(true);
    try {
      await gitSaveCredentials(creds);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      const current = await gitGetCredentials().catch(() => credentials);
      if (current) setCredentials(current);
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = (index: number) => {
    if (!credentials) return;
    const keys = [...credentials.sshKeys];
    keys.splice(index, 1);
    void persist({ ...credentials, sshKeys: keys });
  };

  const handleSave = (config: SshKeyConfig) => {
    if (!credentials) return;
    if (editIndex !== null) {
      const keys = [...credentials.sshKeys];
      keys[editIndex] = config;
      void persist({ ...credentials, sshKeys: keys });
    } else {
      void persist({ ...credentials, sshKeys: [...credentials.sshKeys, config] });
    }
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

  const handleGenerateKey = async () => {
    setKeyGenBusy(true);
    setError(null);
    try {
      const status = await gitGenerateKey();
      setKeyStatus(status);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setKeyGenBusy(false);
    }
  };

  const handleImportKey = async () => {
    try {
      const selected = await open({
        multiple: false,
        title: "Выберите файл приватного SSH ключа",
      });
      if (selected === null || Array.isArray(selected)) return;
      setKeyGenBusy(true);
      setError(null);
      try {
        const status = await gitImportKey(selected);
        setKeyStatus(status);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setKeyGenBusy(false);
      }
    } catch {
      // dialog cancelled
    }
  };

  const handleCopyPublicKey = useCallback(async () => {
    if (!keyStatus?.publicKey) return;
    try {
      await navigator.clipboard.writeText(keyStatus.publicKey);
      setCopyFeedback(true);
      setTimeout(() => setCopyFeedback(false), 2000);
    } catch {
      // clipboard not available
    }
  }, [keyStatus?.publicKey]);

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
    <div className="credentials-tab">
      {/* Section 1: App Key */}
      <div className="credentials-section-title">Ключ приложения</div>
      <p className="credentials-lead">
        Atlas использует Ed25519 SSH ключ для авторизации в Git. 
        Закрытый ключ хранится зашифрованным. Для авторизации приложения в Git, 
        необходимо добавить его в раздел SSH и GPG ключей в вашем аккаунте Bitbucket, GitHub, GitLab и т.д.
      </p>

      {!keyStatus.exists ? (
        <div className="credentials-app-key-none">
          <p className="credentials-empty">
            SSH ключ не настроен. Сгенерируйте новый ключ или импортируйте
            существующий.
          </p>
          <div className="credentials-actions">
            <button
              type="button"
              className="settings-btn primary"
              disabled={keyGenBusy}
              onClick={() => void handleGenerateKey()}
            >
              {keyGenBusy ? "Генерация..." : "Сгенерировать ключ"}
            </button>
            <button
              type="button"
              className="settings-btn"
              disabled={keyGenBusy}
              onClick={() => void handleImportKey()}
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
          <div className="credentials-actions">
            <button
              type="button"
              className="settings-btn primary"
              onClick={handleCopyPublicKey}
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
              onClick={() => void handleImportKey()}
            >
              Импортировать из файла...
            </button>
          </div>
        </div>
      )}

      <hr className="credentials-divider" />

      {/* Section: SSH host key verification */}
      <div className="credentials-section-title">Проверка SSH хостов</div>
      <label className="credentials-checkbox-label">
        <input
          type="checkbox"
          checked={credentials.trustAllSshHostKeys}
          onChange={handleToggleTrustAll}
          disabled={busy}
          className="credentials-checkbox"
        />
        <span>Принимать все SSH сертификаты (Trust-On-First-Use)</span>
      </label>
      <p className="credentials-lead">
        Когда включено, приложение принимает любой SSH ключ хоста при первом
        подключении. Когда выключено, используется стандартная проверка через{" "}
        <code>~/.ssh/known_hosts</code>. Отключите для максимальной
        безопасности, если у вас настроен файл known_hosts.
      </p>

      <hr className="credentials-divider" />
      <div className="credentials-section-title">
        Дополнительные SSH ключи
      </div>
      <p className="credentials-lead">
        Дополнительные SSH ключи для специфичных хостов. Приоритет: ключ приложения,
        затем SSH-агент, затем ключи из этого списка.
      </p>

      {credentials.sshKeys.length === 0 ? (
        <p className="credentials-empty">Нет сохранённых SSH ключей.</p>
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
                  onClick={() => handleDelete(index)}
                >
                  Удалить
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      <div className="credentials-actions">
        <button
          type="button"
          className="settings-btn primary"
          disabled={busy}
          onClick={openAdd}
        >
          Добавить SSH ключ
        </button>
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
                  void handleGenerateKey();
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
