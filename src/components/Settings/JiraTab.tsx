import { useState } from "react";
import { Check, CheckCircle2, ChevronDown, ChevronRight, Save, XCircle } from "lucide-react";
import { useJiraSettings } from "../../hooks/useJiraSettings";
import { CERT_PLACEHOLDER } from "./certField";
import "../Welcome/CloneRepoModal.css";
import "./JiraTab.css";

export function JiraTab() {
  const {
    view,
    tokenSet,
    tokenDraft,
    setTokenDraft,
    tokenSaved,
    busy,
    testing,
    testResult,
    error,
    setField,
    commit,
    saveToken,
    removeToken,
    testConnection,
  } = useJiraSettings();

  // Сертификат нужен один раз при настройке внутреннего инстанса и потом не
  // трогается — свёрнут, как «Дополнительно» на вкладке провайдеров LLM.
  const [advancedOpen, setAdvancedOpen] = useState(false);

  if (!view) {
    return (
      <div className="settings-sections jira-tab">
        {error ? <div className="settings-error">{error}</div> : <p>Загрузка...</p>}
      </div>
    );
  }

  const { settings, bundledBaseUrl, hasBundledCert } = view;

  return (
    <div className="settings-sections jira-tab">
      <div className="settings-card">
        <div className="settings-section-title">Подключение</div>
        <p className="settings-hint settings-hint-compact">
          Приложение авторизуется в Jira по персональному токену доступа
          (Personal Access Token). Токен создаётся в вашем профиле Jira:
          Profile → Personal Access Tokens.
        </p>

        <label className="jira-field">
          <span className="jira-field-label">Адрес Jira</span>
          <input
            className="clone-modal-input"
            type="text"
            placeholder={bundledBaseUrl ?? "https://jira.example.com"}
            value={settings.baseUrl}
            disabled={busy}
            onChange={(event) => setField({ baseUrl: event.target.value })}
            onBlur={() => void commit()}
          />
          <p className="settings-hint settings-hint-compact">
            {bundledBaseUrl
              ? `Адрес задан сборкой приложения (${bundledBaseUrl}). Заполните поле, чтобы использовать другой инстанс.`
              : "Только корень инстанса, без /rest/... и без пути к проекту."}
          </p>
        </label>

        <label className="jira-field">
          <span className="jira-field-label">Токен</span>
          <div className="jira-token-row">
            <input
              className="clone-modal-input"
              type="password"
              placeholder={
                tokenSet ? "Токен сохранён — введите новый, чтобы заменить" : "Вставьте токен"
              }
              value={tokenDraft}
              disabled={busy}
              onChange={(event) => setTokenDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void saveToken();
                }
              }}
              onBlur={() => void saveToken()}
            />
            <button
              type="button"
              className="jira-icon-btn"
              disabled={busy || !tokenDraft.trim()}
              title={tokenSaved ? "Сохранено!" : "Сохранить токен"}
              aria-label={tokenSaved ? "Сохранено!" : "Сохранить токен"}
              onClick={() => void saveToken()}
            >
              {tokenSaved ? <Check size={15} aria-hidden /> : <Save size={15} aria-hidden />}
            </button>
          </div>
          <p className="settings-hint settings-hint-compact">
            Токен хранится зашифрованным в <code>~/.atlas</code> и не возвращается в
            интерфейс — его можно только заменить или удалить.
          </p>
        </label>

        <div className="jira-test-row">
          <button
            type="button"
            className="settings-btn"
            disabled={testing || busy}
            onClick={() => void testConnection()}
          >
            {testing ? "Проверка…" : "Проверить соединение"}
          </button>
          {tokenSet ? (
            <button
              type="button"
              className="settings-link-btn danger"
              disabled={busy}
              onClick={() => void removeToken()}
            >
              Удалить токен
            </button>
          ) : null}
          {!testing && testResult ? (
            <span
              className="jira-test-result"
              title={testResult.ok ? testResult.user.displayName : testResult.message}
            >
              {testResult.ok ? (
                <CheckCircle2 className="ok" size={16} aria-hidden />
              ) : (
                <XCircle className="error" size={16} aria-hidden />
              )}
              <span className={testResult.ok ? "ok" : "error"}>
                {testResult.ok
                  ? `Соединение OK — ${testResult.user.displayName}`
                  : testResult.message}
              </span>
            </span>
          ) : null}
        </div>
      </div>

      <div className="settings-card jira-advanced">
        <button
          type="button"
          className="jira-advanced-toggle"
          aria-expanded={advancedOpen}
          onClick={() => setAdvancedOpen((open) => !open)}
        >
          {advancedOpen ? (
            <ChevronDown size={14} aria-hidden />
          ) : (
            <ChevronRight size={14} aria-hidden />
          )}
          <span>Дополнительно</span>
          <span className="jira-advanced-hint">
            {hasBundledCert ? "сертификат — задан сборкой" : "сертификат"}
          </span>
        </button>

        {advancedOpen ? (
          <div className="jira-advanced-body">
            <label className="jira-field">
              <span className="jira-field-label">
                Доверенный сертификат{hasBundledCert ? " (переопределение)" : ""}
              </span>
              <textarea
                className="clone-modal-input jira-cert-input"
                rows={4}
                spellCheck={false}
                placeholder={CERT_PLACEHOLDER}
                value={settings.trustedCertPem ?? ""}
                disabled={busy}
                onChange={(event) => setField({ trustedCertPem: event.target.value || null })}
                onBlur={() => void commit()}
              />
              <p className="settings-hint settings-hint-compact">
                {hasBundledCert
                  ? "Сертификат центра сертификации задан сборкой приложения — поле пустое, пока вы его не переопределили."
                  : "Если Jira развёрнута за корпоративным центром сертификации, вставьте его сертификат в формате PEM."}{" "}
                Сертификат полностью заменяет публичные корневые сертификаты для
                запросов к Jira; можно вставить цепочку из нескольких сертификатов
                подряд.
              </p>
            </label>
          </div>
        ) : null}
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
