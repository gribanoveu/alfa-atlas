import { useEffect, useState } from "react";
import { useEmbeddingSetup } from "../../hooks/useEmbeddingSetup";
import type { EmbeddingProviderKind } from "../../lib/embeddings";
import "../Welcome/CloneRepoModal.css";
import "./EmbeddingsTab.css";

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

export function EmbeddingsTab() {
  const {
    config,
    modelStatus,
    hasApiKey,
    busy,
    error,
    lastSync,
    indexStatus,
    syncProgress,
    providerConfigured,
    updateConfig,
    saveApiKey,
    downloadModel,
    cancelDownload,
    sync,
  } = useEmbeddingSetup();

  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [dimensions, setDimensions] = useState("");
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [apiKeySaved, setApiKeySaved] = useState(false);
  const [syncing, setSyncing] = useState(false);

  useEffect(() => {
    if (!config) return;
    setBaseUrl(config.remoteBaseUrl ?? "");
    setModel(config.remoteModel ?? "");
    setDimensions(config.remoteDimensions != null ? String(config.remoteDimensions) : "");
  }, [config]);

  if (!config) {
    return (
      <div className="embeddings-tab">
        {error ? <div className="settings-error">{error}</div> : <p>Загрузка...</p>}
      </div>
    );
  }

  const handleSaveApiKey = async () => {
    if (!apiKeyInput.trim()) return;
    await saveApiKey(apiKeyInput.trim());
    setApiKeyInput("");
    setApiKeySaved(true);
    setTimeout(() => setApiKeySaved(false), 2000);
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
    <div className="embeddings-tab">
      <div className="settings-section-title">Провайдер эмбеддингов</div>
      <p className="settings-lead">
        Используется для семантического индекса чанков документации (поиск,
        будущие AI-функции). Смена провайдера действует на всё приложение, а
        не только на текущий проект.
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
        <div className="settings-row">
          <div className="settings-section-title">Модель</div>
          {modelStatus.status === "notDownloaded" ? (
            <>
              <p className="settings-hint" style={{ paddingLeft: 0 }}>
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
        </div>
      ) : (
        <div className="settings-row">
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
            <span className="clone-modal-label">Размерность вектора</span>
            <input
              className="clone-modal-input"
              type="number"
              placeholder="1536 (по умолчанию)"
              value={dimensions}
              disabled={busy}
              onChange={(event) => setDimensions(event.target.value)}
              onBlur={() => {
                const parsed = Number(dimensions);
                void updateConfig({
                  remoteDimensions: dimensions.trim() && Number.isFinite(parsed) ? parsed : null,
                });
              }}
            />
          </label>
          <label className="clone-modal-field">
            <span className="clone-modal-label">API ключ</span>
            <input
              className="clone-modal-input"
              type="password"
              placeholder={hasApiKey ? "Ключ сохранён — введите новый, чтобы заменить" : "sk-..."}
              value={apiKeyInput}
              disabled={busy}
              onChange={(event) => setApiKeyInput(event.target.value)}
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
            {hasApiKey ? (
              <span className="embeddings-status-badge ok">Ключ задан</span>
            ) : (
              <span className="embeddings-status-badge">Ключ не задан</span>
            )}
          </div>
        </div>
      )}

      <hr className="credentials-divider" />

      <div className="settings-row">
        <div className="settings-section-title">Индекс эмбеддингов</div>
        <p className="settings-hint" style={{ paddingLeft: 0 }}>
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
          <p className="settings-hint" style={{ paddingLeft: 0 }}>
            Провайдер ещё не готов — {config.kind === "local"
              ? "загрузите модель"
              : "укажите base URL, модель и API ключ"}.
          </p>
        ) : null}
        {lastSync ? (
          <p className="settings-hint" style={{ paddingLeft: 0 }}>
            Готово: добавлено {lastSync.embedded}, без изменений{" "}
            {lastSync.skippedUnchanged}, удалено {lastSync.removed}.
          </p>
        ) : indexStatus?.stale ? (
          <p className="settings-hint" style={{ paddingLeft: 0 }}>
            Индекс устарел (обновилось приложение) — требуется повторная синхронизация.
          </p>
        ) : indexStatus?.synced ? (
          <p className="settings-hint" style={{ paddingLeft: 0 }}>
            Проиндексировано чанков: {indexStatus.embeddedCount}.
          </p>
        ) : null}
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
