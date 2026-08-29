import { AlertCircle, Check, CheckCircle2, ChevronDown, ChevronRight, RefreshCw, Save, Search, X, XCircle } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { toMessage } from "../../lib/errors";
import { useLlmSetup } from "../../hooks/useLlmSetup";
import { AUTO_MODEL_LABEL, AUTO_MODEL_VALUE, CUSTOM_MODEL_PLACEHOLDER } from "../../lib/assistantConfig";
import {
  DEFAULT_PROVIDER_TOKEN_LIMIT,
  formatLlmRequestHeaders,
  LLM_REQUEST_HEADER_UUID,
  mergeKnownModels,
  parseLlmRequestHeaders,
  resolveOpenAiCompatibleEndpoints,
} from "../../lib/llm";
import "../Welcome/CloneRepoModal.css";
import "./LlmTab.css";

function slugifyProviderId(label: string): string {
  const base = label
    .trim()
    .toLowerCase()
    .replace(/\s+/g, "-")
    .replace(/[^\p{L}\p{N}-]+/gu, "")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "");
  return base || "provider";
}

function uniqueProviderId(label: string, existingIds: string[]): string {
  const base = slugifyProviderId(label);
  if (!existingIds.includes(base)) return base;
  let suffix = 2;
  while (existingIds.includes(`${base}-${suffix}`)) suffix += 1;
  return `${base}-${suffix}`;
}

function effectiveTokenLimit(provider: ProviderDetailProps["provider"]) {
  return {
    context: provider.limit?.context ?? DEFAULT_PROVIDER_TOKEN_LIMIT.context,
    output: provider.limit?.output ?? DEFAULT_PROVIDER_TOKEN_LIMIT.output,
  };
}

function parseTokenLimitField(raw: string): number | null {
  const n = Number.parseInt(raw.trim(), 10);
  if (!Number.isFinite(n) || n < 1) return null;
  return n;
}

function LlmEndpointHint({ baseUrl }: { baseUrl: string | null | undefined }) {
  const endpoints = resolveOpenAiCompatibleEndpoints(baseUrl);
  if (!endpoints) return null;
  return (
    <p className="settings-hint settings-hint-compact llm-endpoint-hint">
      Будут вызваны <span className="llm-endpoint-suffix">{endpoints.chat}</span> и{" "}
      <span className="llm-endpoint-suffix">{endpoints.models}</span>
    </p>
  );
}

type ProviderDetailProps = {
  provider: NonNullable<ReturnType<typeof useLlmSetup>["providers"][number]>;
  isActive: boolean;
  configured: boolean;
  busy: boolean;
  hasApiKey: boolean;
  selectActiveProvider: (id: string) => Promise<void>;
  updateProviderConfig: ReturnType<typeof useLlmSetup>["updateProviderConfig"];
  saveApiKey: (id: string, key: string) => Promise<void>;
  loadModels: (id: string) => Promise<{ id: string }[]>;
  testConnection: (id: string) => Promise<string>;
  removeProvider: (id: string) => Promise<void>;
  onRemoved: () => void;
};

function ProviderDetail({
  provider,
  isActive,
  configured,
  busy,
  hasApiKey,
  selectActiveProvider,
  updateProviderConfig,
  saveApiKey,
  loadModels,
  testConnection,
  removeProvider,
  onRemoved,
}: ProviderDetailProps) {
  const [baseUrlDraft, setBaseUrlDraft] = useState(provider.baseUrl);
  const [headersDraft, setHeadersDraft] = useState(formatLlmRequestHeaders(provider.requestHeaders));
  const [certDraft, setCertDraft] = useState(provider.trustedCertPem ?? "");
  const [contextDraft, setContextDraft] = useState(String(effectiveTokenLimit(provider).context));
  const [outputDraft, setOutputDraft] = useState(String(effectiveTokenLimit(provider).output));
  const [newModelDraft, setNewModelDraft] = useState("");
  const [modelFilterDraft, setModelFilterDraft] = useState("");
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [apiKeySaved, setApiKeySaved] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [modelSelectOpen, setModelSelectOpen] = useState(false);
  const modelSelectRef = useRef<HTMLDivElement>(null);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    setBaseUrlDraft(provider.baseUrl);
    setHeadersDraft(formatLlmRequestHeaders(provider.requestHeaders));
    setNewModelDraft("");
    setModelFilterDraft("");
    setModelsError(null);
    setTestResult(null);
    setAdvancedOpen(false);
    setModelSelectOpen(false);
  }, [provider.id]);

  useEffect(() => {
    setCertDraft(provider.trustedCertPem ?? "");
  }, [provider.id, provider.trustedCertPem]);

  useEffect(() => {
    const limit = effectiveTokenLimit(provider);
    setContextDraft(String(limit.context));
    setOutputDraft(String(limit.output));
  }, [provider.id, provider.limit?.context, provider.limit?.output]);

  useEffect(() => {
    if (!modelSelectOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!modelSelectRef.current?.contains(event.target as Node)) setModelSelectOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      setModelSelectOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [modelSelectOpen]);

  const handleSaveApiKey = async () => {
    if (!apiKeyInput.trim()) return;
    await saveApiKey(provider.id, apiKeyInput.trim());
    setApiKeyInput("");
    setApiKeySaved(true);
    setTimeout(() => setApiKeySaved(false), 2000);
  };

  const handleSelectModel = (value: string) => {
    setModelSelectOpen(false);
    void updateProviderConfig(provider.id, { model: value === AUTO_MODEL_VALUE ? null : value });
  };

  const handleAddModel = async () => {
    const trimmed = newModelDraft.trim();
    if (!trimmed) return;
    const knownModels = mergeKnownModels(provider.knownModels, [trimmed]);
    const patch: { knownModels: string[]; model?: string } = { knownModels };
    if (!provider.model) patch.model = trimmed;
    await updateProviderConfig(provider.id, patch);
    setNewModelDraft("");
  };

  const handleLoadModels = async () => {
    setModelsLoading(true);
    setModelsError(null);
    try {
      const fetched = await loadModels(provider.id);
      await updateProviderConfig(provider.id, {
        knownModels: mergeKnownModels(
          provider.knownModels,
          fetched.map((m) => m.id),
        ),
      });
    } catch (e) {
      setModelsError(toMessage(e));
    } finally {
      setModelsLoading(false);
    }
  };

  const handleTestConnection = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const reply = await testConnection(provider.id);
      setTestResult({ ok: true, message: reply });
    } catch (e) {
      setTestResult({ ok: false, message: toMessage(e) });
    } finally {
      setTesting(false);
    }
  };

  const handleSaveTokenLimits = () => {
    const context = parseTokenLimitField(contextDraft);
    const output = parseTokenLimitField(outputDraft);
    if (context === null || output === null) {
      const limit = effectiveTokenLimit(provider);
      setContextDraft(String(limit.context));
      setOutputDraft(String(limit.output));
      return;
    }
    const current = effectiveTokenLimit(provider);
    if (context === current.context && output === current.output) return;
    void updateProviderConfig(provider.id, { limit: { context, output } });
  };

  const handleResetTokenLimits = () => {
    void updateProviderConfig(provider.id, { limit: null });
  };

  const modelOptions = [
    { value: AUTO_MODEL_VALUE, label: AUTO_MODEL_LABEL },
    ...(provider.model && !provider.knownModels.includes(provider.model)
      ? [{ value: provider.model, label: provider.model }]
      : []),
    ...provider.knownModels.map((id) => ({ value: id, label: id })),
  ];

  const filteredKnownModels = useMemo(() => {
    const query = modelFilterDraft.trim().toLowerCase();
    if (!query) return provider.knownModels;
    return provider.knownModels.filter((id) => id.toLowerCase().includes(query));
  }, [provider.knownModels, modelFilterDraft]);

  return (
    <div className="llm-provider-detail">
      <div className="llm-detail-header">
        <div className="llm-detail-summary">
          <span className={`llm-detail-pill${configured ? " ok" : " pending"}`}>
            {configured ? "Ключ сохранён" : "Нужен API ключ"}
          </span>
          {isActive ? <span className="llm-detail-pill active">Используется в чате</span> : null}
        </div>
        {!isActive ? (
          <button
            type="button"
            className="settings-btn primary llm-detail-use-btn"
            disabled={busy}
            onClick={() => void selectActiveProvider(provider.id)}
          >
            Использовать для чата
          </button>
        ) : null}
      </div>

      <section className="llm-detail-group">
        <h4 className="llm-detail-group-title">Подключение</h4>

        <label className="llm-field">
          <span className="llm-field-label">API ключ</span>
          <div className="llm-api-key-row">
            <input
              className="clone-modal-input"
              type="password"
              placeholder={hasApiKey ? "Ключ сохранён — введите новый, чтобы заменить" : "sk-..."}
              value={apiKeyInput}
              disabled={busy}
              onChange={(event) => setApiKeyInput(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void handleSaveApiKey();
                }
              }}
            />
            <button
              type="button"
              className="llm-icon-btn"
              disabled={busy || !apiKeyInput.trim()}
              title={apiKeySaved ? "Сохранено!" : "Сохранить ключ"}
              aria-label={apiKeySaved ? "Сохранено!" : "Сохранить ключ"}
              onClick={() => void handleSaveApiKey()}
            >
              {apiKeySaved ? <Check size={15} aria-hidden /> : <Save size={15} aria-hidden />}
            </button>
          </div>
        </label>

        {provider.isSystem ? (
          <div className="llm-field">
            <span className="llm-field-label">Base URL</span>
            <p className="llm-readonly-value">{provider.baseUrl}</p>
            <p className="settings-hint settings-hint-compact">Задаётся сборкой приложения.</p>
          </div>
        ) : (
          <label className="llm-field">
            <span className="llm-field-label">Base URL</span>
            <input
              className="clone-modal-input"
              type="text"
              placeholder="https://api.openai.com/v1"
              value={baseUrlDraft}
              disabled={busy}
              onChange={(event) => setBaseUrlDraft(event.target.value)}
              onBlur={() => void updateProviderConfig(provider.id, { baseUrl: baseUrlDraft.trim() || null })}
            />
            <LlmEndpointHint baseUrl={baseUrlDraft.trim() || provider.baseUrl} />
          </label>
        )}

        <div className="llm-test-row">
          <button
            type="button"
            className="settings-btn"
            disabled={testing || busy}
            onClick={() => void handleTestConnection()}
          >
            {testing ? "Проверка…" : "Проверить соединение"}
          </button>
          {!testing && testResult ? (
            <span className="llm-test-result" title={testResult.message}>
              {testResult.ok ? (
                <CheckCircle2 className="ok" size={16} aria-hidden />
              ) : (
                <XCircle className="error" size={16} aria-hidden />
              )}
              <span className={testResult.ok ? "ok" : "error"}>
                {testResult.ok ? "Соединение OK" : "Ошибка"}
              </span>
            </span>
          ) : null}
        </div>
      </section>

      <section className="llm-detail-group">
        <h4 className="llm-detail-group-title">Модель</h4>

        <div className="llm-field">
          <span className="llm-field-label" id={`llm-model-label-${provider.id}`}>
            Активная модель
          </span>
          <div className="clone-select llm-model-select" ref={modelSelectRef}>
            <button
              type="button"
              className={`clone-select-trigger${modelSelectOpen ? " is-open" : ""}`}
              aria-haspopup="listbox"
              aria-expanded={modelSelectOpen}
              aria-labelledby={`llm-model-label-${provider.id}`}
              disabled={busy}
              onClick={() => setModelSelectOpen((open) => !open)}
            >
              <span className="clone-select-value">
                <span className="clone-select-path">{provider.model ?? AUTO_MODEL_LABEL}</span>
              </span>
              <span className="clone-select-chevron" aria-hidden>
                ▾
              </span>
            </button>
            {modelSelectOpen ? (
              <div className="clone-select-menu" role="listbox">
                {modelOptions.map((option) => {
                  const selected =
                    option.value === AUTO_MODEL_VALUE ? !provider.model : option.value === provider.model;
                  return (
                    <button
                      key={option.value}
                      type="button"
                      role="option"
                      aria-selected={selected}
                      className={`clone-select-option${selected ? " is-active" : ""}`}
                      onClick={() => handleSelectModel(option.value)}
                    >
                      <span className="clone-select-path">{option.label}</span>
                    </button>
                  );
                })}
              </div>
            ) : null}
          </div>
          <p className="settings-hint settings-hint-compact">
            «Авто» — при первом запросе выбирается первая модель из API и запоминается.
          </p>
        </div>

        <div className="llm-model-toolbar">
          <input
            className="clone-modal-input"
            type="text"
            placeholder={CUSTOM_MODEL_PLACEHOLDER}
            value={newModelDraft}
            disabled={busy}
            onChange={(event) => setNewModelDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void handleAddModel();
              }
            }}
          />
          <button
            type="button"
            className="settings-btn"
            disabled={busy || !newModelDraft.trim()}
            onClick={() => void handleAddModel()}
          >
            Добавить
          </button>
          <button
            type="button"
            className="llm-icon-btn"
            disabled={modelsLoading || busy}
            title="Загрузить модели с API"
            aria-label="Загрузить модели с API"
            onClick={() => void handleLoadModels()}
          >
            <RefreshCw size={15} className={modelsLoading ? "spin" : ""} aria-hidden />
          </button>
        </div>

        {modelsError ? <p className="settings-hint llm-inline-error">{modelsError}</p> : null}

        {provider.knownModels.length > 0 ? (
          <div className="llm-model-catalog">
            <div className="llm-model-search">
              <Search size={13} className="llm-model-search-icon" aria-hidden />
              <input
                type="search"
                className="llm-model-search-input"
                placeholder="Поиск по каталогу…"
                aria-label="Поиск по каталогу моделей"
                value={modelFilterDraft}
                disabled={busy}
                onChange={(event) => setModelFilterDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Escape" && modelFilterDraft) {
                    event.stopPropagation();
                    setModelFilterDraft("");
                  }
                }}
              />
              {modelFilterDraft ? (
                <button
                  type="button"
                  className="llm-model-search-clear"
                  aria-label="Очистить поиск"
                  disabled={busy}
                  onClick={() => setModelFilterDraft("")}
                >
                  <X size={12} aria-hidden />
                </button>
              ) : null}
            </div>
            {filteredKnownModels.length > 0 ? (
              <div className="llm-model-chips" role="list" aria-label="Каталог моделей">
                {filteredKnownModels.map((id) => {
                  const selected = id === provider.model;
                  return (
                    <button
                      key={id}
                      type="button"
                      role="listitem"
                      className={`llm-model-chip${selected ? " is-active" : ""}`}
                      aria-pressed={selected}
                      disabled={busy}
                      onClick={() => handleSelectModel(id)}
                    >
                      {id}
                    </button>
                  );
                })}
              </div>
            ) : (
              <p className="settings-hint settings-hint-compact">Ничего не найдено.</p>
            )}
          </div>
        ) : (
          <p className="settings-hint settings-hint-compact">Каталог пуст — добавьте модель или загрузите с API.</p>
        )}
      </section>

      <section className="llm-detail-group">
        <h4 className="llm-detail-group-title">Контекст</h4>
        <div className="llm-limit-row">
          <label className="llm-field">
            <span className="llm-field-label">Контекстное окно (токены)</span>
            <input
              className="clone-modal-input"
              type="number"
              min={1}
              step={1000}
              placeholder={String(DEFAULT_PROVIDER_TOKEN_LIMIT.context)}
              value={contextDraft}
              disabled={busy}
              onChange={(event) => setContextDraft(event.target.value)}
              onBlur={handleSaveTokenLimits}
            />
          </label>
          <label className="llm-field">
            <span className="llm-field-label">Макс. ответ (токены)</span>
            <input
              className="clone-modal-input"
              type="number"
              min={1}
              step={1000}
              placeholder={String(DEFAULT_PROVIDER_TOKEN_LIMIT.output)}
              value={outputDraft}
              disabled={busy}
              onChange={(event) => setOutputDraft(event.target.value)}
              onBlur={handleSaveTokenLimits}
            />
          </label>
        </div>
        <p className="settings-hint settings-hint-compact">
          По умолчанию — {DEFAULT_PROVIDER_TOKEN_LIMIT.context.toLocaleString("ru-RU")}. Используется для
          счётчика контекста в чате и авто-сжатия истории.
        </p>
        <div className="llm-limit-actions">
          <button type="button" className="settings-btn" disabled={busy} onClick={handleResetTokenLimits}>
            Сбросить
          </button>
        </div>
      </section>

      <section className="llm-detail-group llm-detail-advanced">
        <button
          type="button"
          className="llm-advanced-toggle"
          aria-expanded={advancedOpen}
          onClick={() => setAdvancedOpen((open) => !open)}
        >
          {advancedOpen ? (
            <ChevronDown size={14} aria-hidden />
          ) : (
            <ChevronRight size={14} aria-hidden />
          )}
          <span>Дополнительно</span>
          <span className="llm-advanced-hint">заголовки, сертификат</span>
        </button>

        {advancedOpen ? (
          <div className="llm-advanced-body">
            <label className="llm-field">
              <span className="llm-field-label">HTTP-заголовки</span>
              <textarea
                className="llm-textarea"
                rows={3}
                placeholder={`systemId: sanduser\nmessageId: ${LLM_REQUEST_HEADER_UUID}`}
                value={headersDraft}
                disabled={busy}
                onChange={(event) => setHeadersDraft(event.target.value)}
                onBlur={() =>
                  void updateProviderConfig(provider.id, {
                    requestHeaders: parseLlmRequestHeaders(headersDraft),
                  })
                }
              />
              <p className="settings-hint settings-hint-compact">
                По строке: <code>Имя: значение</code>. <code>{LLM_REQUEST_HEADER_UUID}</code> — новый UUID на запрос.
              </p>
            </label>

            <label className="llm-field">
              <span className="llm-field-label">
                Доверенный сертификат{provider.isSystem ? " (переопределение)" : ""}
              </span>
              <textarea
                className="llm-textarea"
                rows={4}
                placeholder={
                  provider.isSystem
                    ? "Встроенный сертификат задан. Вставьте PEM, чтобы переопределить."
                    : "PEM сертификата CA, если эндпоинт не доверен системой."
                }
                value={certDraft}
                disabled={busy}
                onChange={(event) => setCertDraft(event.target.value)}
              />
              <div className="llm-advanced-actions">
                <button
                  type="button"
                  className="settings-btn primary"
                  disabled={busy || !certDraft.trim()}
                  onClick={() =>
                    void updateProviderConfig(provider.id, { trustedCertPem: certDraft.trim() || null })
                  }
                >
                  Сохранить сертификат
                </button>
                {provider.isSystem ? (
                  <button
                    type="button"
                    className="settings-btn"
                    disabled={busy}
                    onClick={() => void updateProviderConfig(provider.id, { trustedCertPem: null })}
                  >
                    Сбросить
                  </button>
                ) : null}
              </div>
            </label>
          </div>
        ) : null}
      </section>

      {!provider.isSystem ? (
        <div className="llm-detail-footer">
          <button
            type="button"
            className="settings-link-btn danger"
            disabled={busy}
            onClick={() => {
              onRemoved();
              void removeProvider(provider.id);
            }}
          >
            Удалить провайдера
          </button>
        </div>
      ) : null}
    </div>
  );
}

export function LlmTab() {
  const {
    settings,
    providers,
    hasApiKeyMap,
    busy,
    error,
    selectActiveProvider,
    updateProviderConfig,
    removeProvider,
    saveApiKey,
    loadModels,
    testConnection,
    setRateLimitEnabled,
  } = useLlmSetup();

  const activeId = settings?.activeProviderId ?? providers[0]?.id ?? null;
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [newProviderLabel, setNewProviderLabel] = useState("");
  const [newProviderBaseUrl, setNewProviderBaseUrl] = useState("");

  const handleAddProvider = async () => {
    const label = newProviderLabel.trim();
    const baseUrl = newProviderBaseUrl.trim();
    if (!label || !baseUrl) return;
    const id = uniqueProviderId(
      label,
      providers.map((p) => p.id),
    );
    await updateProviderConfig(id, { label, baseUrl });
    setNewProviderLabel("");
    setNewProviderBaseUrl("");
    setExpandedId(id);
  };

  return (
    <div className="settings-sections llm-tab">
      <div className="llm-provider-list">
        {providers.map((provider) => {
          const isOpen = provider.id === expandedId;
          const isActive = provider.id === activeId;
          const configured = hasApiKeyMap[provider.id] === true;
          const Chevron = isOpen ? ChevronDown : ChevronRight;
          return (
            <div key={provider.id} className={`llm-provider-item${isOpen ? " is-open" : ""}`}>
              <button
                type="button"
                className="llm-provider-row"
                aria-expanded={isOpen}
                onClick={() => setExpandedId(isOpen ? null : provider.id)}
              >
                <Chevron className="llm-provider-chevron" size={15} aria-hidden />
                {configured ? (
                  <CheckCircle2 className="llm-provider-status ok" size={16} aria-hidden />
                ) : (
                  <AlertCircle className="llm-provider-status pending" size={16} aria-hidden />
                )}
                <span className="llm-provider-row-label">{provider.label}</span>
                <span className={`llm-provider-badge${provider.isSystem ? " system" : " custom"}`}>
                  {provider.isSystem ? "встроенный" : "свой"}
                </span>
                {isActive ? <span className="llm-provider-badge active">активен</span> : null}
              </button>

              {isOpen ? (
                <ProviderDetail
                  provider={provider}
                  isActive={isActive}
                  configured={configured}
                  busy={busy}
                  hasApiKey={hasApiKeyMap[provider.id] === true}
                  selectActiveProvider={selectActiveProvider}
                  updateProviderConfig={updateProviderConfig}
                  saveApiKey={saveApiKey}
                  loadModels={loadModels}
                  testConnection={testConnection}
                  removeProvider={removeProvider}
                  onRemoved={() => setExpandedId(null)}
                />
              ) : null}
            </div>
          );
        })}
        {providers.length === 0 ? (
          <p className="settings-hint settings-hint-compact">Провайдеры не настроены.</p>
        ) : null}
      </div>

      <div className="settings-card llm-add-provider">
        <div className="settings-section-title">Добавить провайдера</div>
        <p className="settings-hint settings-hint-compact">
          OpenAI-совместимый API — укажите название и корень (<span className="llm-endpoint-suffix">…/v1</span>).
          Ключ и модель настраиваются после добавления.
        </p>
        <label className="llm-field">
          <span className="llm-field-label">Название</span>
          <input
            className="clone-modal-input"
            type="text"
            placeholder="Мой провайдер"
            value={newProviderLabel}
            onChange={(event) => setNewProviderLabel(event.target.value)}
          />
        </label>
        <label className="llm-field">
          <span className="llm-field-label">Base URL</span>
          <input
            className="clone-modal-input"
            type="text"
            placeholder="https://api.openai.com/v1"
            value={newProviderBaseUrl}
            onChange={(event) => setNewProviderBaseUrl(event.target.value)}
          />
          <LlmEndpointHint baseUrl={newProviderBaseUrl} />
        </label>
        <div className="settings-actions">
          <button
            type="button"
            className="settings-btn primary"
            disabled={busy || !newProviderLabel.trim() || !newProviderBaseUrl.trim()}
            onClick={() => void handleAddProvider()}
          >
            Добавить провайдера
          </button>
        </div>
      </div>

      <div className="settings-card">
        <div className="settings-section-title">Лимиты API</div>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={settings?.rateLimitEnabled ?? true}
            disabled={busy || !settings}
            onChange={(event) => void setRateLimitEnabled(event.target.checked)}
          />
          <span>Учитывать лимиты API</span>
        </label>
        <p className="settings-hint">
          Для AlfaGen — скользящее окно EVC. Выключите, чтобы скрыть чип и не записывать расход токенов.
        </p>
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
