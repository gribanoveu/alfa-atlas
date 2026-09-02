import { useEffect, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useEmbeddingSetup } from "../../hooks/useEmbeddingSetup";
import { CERT_PLACEHOLDER } from "./certField";
import {
  EMBEDDING_REQUEST_HEADER_UUID,
  formatRequestHeaders,
  parseRequestHeaders,
  testEmbeddingConnection,
  type EmbeddingProviderKind,
} from "../../lib/embeddings";
import { toMessage } from "../../lib/errors";
import "../Welcome/CloneRepoModal.css";
import "./EmbeddingsTab.css";

// Mirrors `commands::repo_index::language_label`'s wire labels.
const LANGUAGE_LABELS: Record<string, string> = {
  java: "Java",
  json: "JSON",
  yaml: "YAML",
  markdown: "Markdown",
  asciidoc: "AsciiDoc",
};

/** "Java: 12, JSON: 5" — sorted by count descending so the dominant
 * language in the repo reads first. */
function describeByLanguage(byLanguage: Record<string, number>): string {
  return Object.entries(byLanguage)
    .sort(([, a], [, b]) => b - a)
    .map(([lang, count]) => `${LANGUAGE_LABELS[lang] ?? lang}: ${count}`)
    .join(", ");
}

const PROVIDER_OPTIONS: { value: EmbeddingProviderKind; label: string; hint: string }[] = [
  {
    value: "local",
    label: "Локально",
    hint: "BGE-M3 (int8, ONNX), выполняется на устройстве. Модель ~570 МБ, загружается один раз.",
  },
  {
    value: "remote",
    label: "Внешний API",
    hint: "OpenAI-совместимый эндпоинт /embeddings (OpenAI, Together, Mistral, локальный Ollama/LM Studio и т.п.).",
  },
];

type EmbeddingsTabProps = {
  repoRoot: string | null;
};

export function EmbeddingsTab({ repoRoot }: EmbeddingsTabProps) {
  const {
    config,
    modelStatus,
    hasApiKey,
    busy,
    error,
    lastSync,
    indexStatus,
    repoIndexSummary,
    syncProgress,
    providerConfigured,
    updateConfig,
    saveApiKey,
    deleteApiKey,
    downloadModel,
    cancelDownload,
    sync,
  } = useEmbeddingSetup(repoRoot);

  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [headersDraft, setHeadersDraft] = useState("");
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [apiKeySaved, setApiKeySaved] = useState(false);
  const [syncing, setSyncing] = useState(false);
  // Только пользовательский слой, не разрешённое значение — см. комментарий
  // к `remoteTrustedCertPem` в `lib/embeddings.ts`.
  const [certDraft, setCertDraft] = useState("");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null);

  useEffect(() => {
    if (!config) return;
    setBaseUrl(config.remoteBaseUrl ?? "");
    setModel(config.remoteModel ?? "");
    setHeadersDraft(formatRequestHeaders(config.remoteRequestHeaders));
    setCertDraft(config.remoteTrustedCertOverride ?? "");
  }, [config]);

  if (!config) {
    return (
      <div className="embeddings-tab">
        {error ? <div className="settings-error">{error}</div> : <p>Загрузка...</p>}
      </div>
    );
  }

  const handleTestConnection = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      setTestResult({ ok: true, message: await testEmbeddingConnection() });
    } catch (e) {
      setTestResult({ ok: false, message: toMessage(e) });
    } finally {
      setTesting(false);
    }
  };

  const apiKeyPlaceholder = config.apiKeyUserSet
    ? "Свой ключ сохранён — введите новый, чтобы заменить"
    : config.apiKeyBundled
      ? "Используется ключ из сборки — введите свой, чтобы переопределить"
      : hasApiKey
        ? "Ключ сохранён — введите новый, чтобы заменить"
        : "sk-...";

  /** Вызывается по потере фокуса и по Enter, а не только кнопкой: ключ,
   * набранный и оставленный в поле, иначе молча пропадал. Пустая строка
   * отсекается здесь же, поэтому уход с нетронутого поля ничего не пишет, а
   * клик по кнопке не сохраняет дважды — blur успевает очистить поле, и к
   * моменту клика кнопка уже неактивна. */
  const handleSaveApiKey = async () => {
    if (!apiKeyInput.trim()) return;
    await saveApiKey(apiKeyInput.trim());
    setApiKeyInput("");
    setApiKeySaved(true);
    setTimeout(() => setApiKeySaved(false), 2000);
  };

  // Also clears whatever was typed but not saved: leaving a half-entered
  // key in the field right after "ключ удалён" reads as if something is
  // still stored.
  const handleDeleteApiKey = async () => {
    await deleteApiKey();
    setApiKeyInput("");
    setApiKeySaved(false);
  };

  const handleSync = async () => {
    setSyncing(true);
    try {
      await sync();
    } finally {
      setSyncing(false);
    }
  };

  return (
    <div className="settings-sections embeddings-tab">
      <div className="settings-card">
      <div className="settings-section-title">Провайдер</div>
      <p className="settings-hint settings-hint-compact">
        Смена провайдера действует на всё приложение, а не только на текущий
        проект.
      </p>
      <div className="embeddings-provider-options">
        {PROVIDER_OPTIONS.map((option) => (
          <label key={option.value} className="embeddings-provider-option">
            <input
              type="radio"
              name="embedding-provider-kind"
              checked={config.kind === option.value}
              disabled={busy}
              onChange={() => void updateConfig({ kind: option.value })}
            />
            <span className="embeddings-provider-option-body">
              <span className="embeddings-provider-option-label">{option.label}</span>
              <span className="embeddings-provider-option-hint">{option.hint}</span>
            </span>
          </label>
        ))}
      </div>

      {config.kind === "local" ? (
        <>
          <hr className="settings-card-divider" />
          <div className="settings-section-title">Модель</div>
          {modelStatus.status === "notDownloaded" ? (
            <>
              <p className="settings-hint settings-hint-compact">
                Модель ещё не загружена.
              </p>
              <div className="settings-actions">
                <button
                  type="button"
                  className="settings-btn primary"
                  disabled={busy}
                  onClick={() => void downloadModel()}
                >
                  Скачать модель (~570 МБ)
                </button>
              </div>
            </>
          ) : null}
          {modelStatus.status === "downloading" ? (
            <div className="embeddings-progress">
              <div className="embeddings-progress-track">
                <div
                  className="embeddings-progress-fill"
                  style={{ width: `${Math.round(modelStatus.progress * 100)}%` }}
                />
              </div>
              <div className="embeddings-progress-row">
                <span className="embeddings-progress-label">
                  Загрузка модели… {Math.round(modelStatus.progress * 100)}%
                </span>
                <button
                  type="button"
                  className="settings-link-btn danger"
                  onClick={() => void cancelDownload()}
                >
                  Отменить
                </button>
              </div>
            </div>
          ) : null}
          {modelStatus.status === "ready" ? (
            <span className="embeddings-status-badge ok">Модель готова</span>
          ) : null}
          {modelStatus.status === "error" ? (
            <>
              <span className="embeddings-status-badge error">
                Ошибка: {modelStatus.message}
              </span>
              <div className="settings-actions">
                <button
                  type="button"
                  className="settings-btn"
                  disabled={busy}
                  onClick={() => void downloadModel()}
                >
                  Повторить загрузку
                </button>
              </div>
            </>
          ) : null}
        </>
      ) : (
        <>
          <hr className="settings-card-divider" />
          <div className="settings-section-title">Параметры эндпоинта</div>
          <label className="clone-modal-field">
            <span className="clone-modal-label">Base URL</span>
            <input
              className="clone-modal-input"
              type="text"
              placeholder="https://api.openai.com/v1"
              value={baseUrl}
              disabled={busy}
              onChange={(event) => setBaseUrl(event.target.value)}
              onBlur={() => void updateConfig({ remoteBaseUrl: baseUrl.trim() || null })}
            />
          </label>
          <label className="clone-modal-field">
            <span className="clone-modal-label">Модель</span>
            <input
              className="clone-modal-input"
              type="text"
              placeholder="text-embedding-3-small"
              value={model}
              disabled={busy}
              onChange={(event) => setModel(event.target.value)}
              onBlur={() => void updateConfig({ remoteModel: model.trim() || null })}
            />
          </label>
          <label className="clone-modal-field">
            <span className="clone-modal-label">HTTP-заголовки</span>
            <textarea
              className="embeddings-config-textarea"
              placeholder={`systemId: sanduser\nmessageId: ${EMBEDDING_REQUEST_HEADER_UUID}`}
              value={headersDraft}
              disabled={busy}
              onChange={(event) => setHeadersDraft(event.target.value)}
              onBlur={() =>
                void updateConfig({ remoteRequestHeaders: parseRequestHeaders(headersDraft) })
              }
            />
          </label>
          <p className="settings-hint settings-hint-compact">
            По одному заголовку на строку: <code>Имя: значение</code>. Значение{" "}
            <code>{EMBEDDING_REQUEST_HEADER_UUID}</code> подставляет новый UUID на каждый запрос.
            Пустое поле — наследовать из bundled-конфига.
          </p>
          {/* Поле показывается всегда, даже когда ключ вшит в сборку: вшитый
              ключ — это дефолт, а не запрет, и свой ключ его перекрывает
              (см. `infra::embedding_credentials_store::get_api_key`). Раньше
              поле в этом случае просто пряталось, и переопределить ключ было
              нечем. */}
          <label className="clone-modal-field">
            <span className="clone-modal-label">API ключ</span>
            <input
              className="clone-modal-input"
              type="password"
              placeholder={apiKeyPlaceholder}
              value={apiKeyInput}
              disabled={busy}
              onChange={(event) => setApiKeyInput(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void handleSaveApiKey();
                }
              }}
              onBlur={() => void handleSaveApiKey()}
            />
          </label>
          <div className="settings-actions">
            <button
              type="button"
              className="settings-btn primary"
              disabled={busy || !apiKeyInput.trim()}
              onClick={() => void handleSaveApiKey()}
            >
              {apiKeySaved ? "Сохранено!" : "Сохранить ключ"}
            </button>
            {/* Удалять можно только свой ключ: вшитый в сборку отсюда не
                убрать, и кнопка на нём вводила бы в заблуждение. */}
            {config.apiKeyUserSet && (
              <button
                type="button"
                className="settings-btn"
                disabled={busy}
                onClick={() => void handleDeleteApiKey()}
              >
                {config.apiKeyBundled ? "Вернуть ключ сборки" : "Удалить ключ"}
              </button>
            )}
            {config.apiKeyUserSet ? (
              <span className="embeddings-status-badge ok">Задан свой ключ</span>
            ) : config.apiKeyBundled ? (
              <span className="embeddings-status-badge ok">Ключ встроен в сборку</span>
            ) : hasApiKey ? (
              <span className="embeddings-status-badge ok">Ключ задан</span>
            ) : (
              <span className="embeddings-status-badge">Ключ не задан</span>
            )}
          </div>

          <div className="settings-actions embeddings-test-row">
            <button
              type="button"
              className="settings-btn"
              disabled={busy || testing}
              onClick={() => void handleTestConnection()}
            >
              {testing ? "Проверка…" : "Проверить соединение"}
            </button>
            {!testing && testResult ? (
              <span
                className={`embeddings-status-badge ${testResult.ok ? "ok" : "error"}`}
                title={testResult.message}
              >
                {testResult.message}
              </span>
            ) : null}
          </div>
          <p className="settings-hint settings-hint-compact">
            Отправляет одно короткое слово на эндпоинт и сверяет размерность
            ответа с той, на которую рассчитан индекс.
          </p>

          {/* Сертификат правится один раз при настройке внутреннего эндпоинта
              — свёрнут, как «Дополнительно» на вкладке провайдеров LLM. */}
          <div className="embeddings-advanced">
            <button
              type="button"
              className="embeddings-advanced-toggle"
              aria-expanded={advancedOpen}
              onClick={() => setAdvancedOpen((open) => !open)}
            >
              {advancedOpen ? (
                <ChevronDown size={14} aria-hidden />
              ) : (
                <ChevronRight size={14} aria-hidden />
              )}
              <span>Дополнительно</span>
              <span className="embeddings-advanced-hint">
                {config.hasBundledCert ? "сертификат — задан сборкой" : "сертификат"}
              </span>
            </button>

            {advancedOpen ? (
              <div className="embeddings-advanced-body">
                <label className="clone-modal-field">
                  <span className="clone-modal-label">
                    Доверенный сертификат{config.hasBundledCert ? " (переопределение)" : ""}
                  </span>
                  <textarea
                    className="embeddings-config-textarea"
                    rows={4}
                    spellCheck={false}
                    placeholder={CERT_PLACEHOLDER}
                    value={certDraft}
                    disabled={busy}
                    onChange={(event) => setCertDraft(event.target.value)}
                    onBlur={() =>
                      void updateConfig({ remoteTrustedCertOverride: certDraft.trim() || null })
                    }
                  />
                  <p className="settings-hint settings-hint-compact">
                    {config.hasBundledCert
                      ? "Сертификат задан сборкой приложения — поле пустое, пока вы его не переопределили."
                      : "Сертификат полностью заменяет публичные корневые сертификаты для запросов к эндпоинту эмбеддингов."}{" "}
                    Можно вставить цепочку из нескольких сертификатов подряд.
                  </p>
                </label>
              </div>
            ) : null}
          </div>
        </>
      )}

      </div>

      <div className="settings-card">
        <div className="settings-section-title">Индекс эмбеддингов</div>
        <p className="settings-hint settings-hint-compact">
          Пересчитывает Chunk Index для текущего проекта и обновляет векторы:
          новые чанки — добавляются, изменённые — пересчитываются, удалённые —
          убираются из индекса.
        </p>
        <div className="settings-actions">
          <button
            type="button"
            className="settings-btn primary"
            disabled={busy || syncing || !providerConfigured}
            onClick={() => void handleSync()}
          >
            {syncing
              ? syncProgress
                ? `${syncProgress.phase === "chunking" ? "Индексация" : "Эмбеддинг"} ${syncProgress.current}/${syncProgress.total}`
                : "Синхронизация…"
              : "Синхронизировать индекс"}
          </button>
        </div>
        {!providerConfigured ? (
          <p className="settings-hint settings-hint-compact">
            Провайдер ещё не готов — {config.kind === "local"
              ? "загрузите модель"
              : "укажите base URL, модель и API ключ"}.
          </p>
        ) : null}
        {lastSync ? (
          <p className="settings-hint settings-hint-compact">
            Готово: добавлено {lastSync.embedded}, без изменений{" "}
            {lastSync.skippedUnchanged}, удалено {lastSync.removed}.
            {indexStatus && indexStatus.backgroundPending > 0
              ? ` Индексация остальной части репозитория продолжается в фоне (осталось файлов: ${indexStatus.backgroundPending}).`
              : null}
          </p>
        ) : indexStatus?.stale ? (
          <p className="settings-hint settings-hint-compact">
            Индекс устарел (обновилось приложение) — требуется повторная синхронизация.
          </p>
        ) : indexStatus?.synced ? (
          <button
            type="button"
            className="settings-hint settings-hint-button settings-hint-compact"
            disabled={busy || syncing || !providerConfigured}
            title="Нажмите, чтобы синхронизировать снова"
            onClick={() => void handleSync()}
          >
            Проиндексировано чанков: {indexStatus.embeddedCount}.
            {indexStatus.backgroundPending > 0
              ? ` Индексация остальной части репозитория продолжается в фоне (осталось файлов: ${indexStatus.backgroundPending}).`
              : null}
            {repoIndexSummary && repoIndexSummary.filesIndexed > 0
              ? ` Файлов: ${repoIndexSummary.filesIndexed} (${describeByLanguage(repoIndexSummary.byLanguage)}).`
              : null}
          </button>
        ) : null}
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
