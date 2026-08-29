import { AlertCircle, Check, CheckCircle2, ChevronDown, ChevronRight, RefreshCw, Save, XCircle } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toMessage } from "../../lib/errors";
import { useLlmSetup } from "../../hooks/useLlmSetup";
import { AUTO_MODEL_LABEL, AUTO_MODEL_VALUE, CUSTOM_MODEL_HINT, CUSTOM_MODEL_PLACEHOLDER } from "../../lib/assistantConfig";
import { mergeKnownModels, resolveOpenAiCompatibleEndpoints } from "../../lib/llm";
import "../Welcome/CloneRepoModal.css";
import "./LlmTab.css";

/** Derives a stable settings-key id from a user-typed label — the "Добавить
 * провайдера" form only asks for a name, not a raw identifier, so this is
 * the one place an id ever gets minted. */
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

function LlmEndpointPreview({ baseUrl }: { baseUrl: string | null | undefined }) {
  const endpoints = resolveOpenAiCompatibleEndpoints(baseUrl);
  if (!endpoints) return null;
  return (
    <div className="llm-endpoint-preview" aria-live="polite">
      <p className="llm-endpoint-preview-title">Итоговые адреса</p>
      <p className="settings-hint settings-hint-compact llm-endpoint-preview-hint">
        Вычисляются из Base URL — редактировать их здесь нельзя. Укажите только корень API без{" "}
        <span className="llm-endpoint-suffix">/chat/completions</span>.
      </p>
      <dl className="llm-endpoint-list">
        <div className="llm-endpoint-item">
          <dt className="llm-endpoint-label">Чат</dt>
          <dd className="llm-endpoint-url">{endpoints.chat}</dd>
        </div>
        <div className="llm-endpoint-item">
          <dt className="llm-endpoint-label">Модели</dt>
          <dd className="llm-endpoint-url">{endpoints.models}</dd>
        </div>
      </dl>
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

  // Accordion: at most one provider's settings body is open at a time —
  // separate from `activeId` (which provider chat actually uses), so
  // reviewing/editing a provider's config no longer has the side effect of
  // switching the app to it.
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const expanded = providers.find((p) => p.id === expandedId) ?? null;

  const [baseUrlDraft, setBaseUrlDraft] = useState("");
  const [newModelDraft, setNewModelDraft] = useState("");
  const [certDraft, setCertDraft] = useState("");
  const [certOpen, setCertOpen] = useState(false);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [apiKeySaved, setApiKeySaved] = useState(false);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null);
  const [testing, setTesting] = useState(false);

  const [newProviderLabel, setNewProviderLabel] = useState("");
  const [newProviderBaseUrl, setNewProviderBaseUrl] = useState("");

  // Programmatic dropdown (trigger button + absolute option list), not a
  // native `<select>` — same `.clone-select*` pattern `SettingsDialog.tsx`
  // already uses for its "Язык сообщений об ошибках" picker, so this looks
  // and behaves consistently with the rest of Settings rather than falling
  // back to the OS's own select styling.
  const [modelSelectOpen, setModelSelectOpen] = useState(false);
  const modelSelectRef = useRef<HTMLDivElement>(null);

  // Re-sync drafts only when the *expanded provider* changes, not on every
  // background `refresh()` — otherwise a save-triggered refresh would wipe
  // whatever the user is mid-typing before their own change round-trips
  // back (harmless here since it round-trips to the same value, but this
  // keeps the intent explicit).
  useEffect(() => {
    setBaseUrlDraft(expanded?.baseUrl ?? "");
    setNewModelDraft("");
    setModelsError(null);
    setTestResult(null);
    setModelSelectOpen(false);
    setCertOpen(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [expanded?.id]);

  // Separate from the effect above: the cert draft must also resync when
  // the *resolved* cert value itself changes for the same provider, not
  // just on provider switch — "Сбросить к встроенному" saves `null` (an
  // intentional divergence from whatever's currently in the draft, unlike
  // every other save here, which round-trips the same value the user just
  // typed) and relies on this effect to then show the restored built-in
  // certificate once `providers` refreshes, instead of leaving the
  // textarea on whatever it was set to right before the save.
  useEffect(() => {
    setCertDraft(expanded?.trustedCertPem ?? "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [expanded?.id, expanded?.trustedCertPem]);

  useEffect(() => {
    if (!modelSelectOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!modelSelectRef.current?.contains(event.target as Node)) setModelSelectOpen(false);
    };
    // Capture phase + `stopPropagation`, so Escape closes the dropdown without
    // also reaching the Settings dialog's own bubble-phase Escape handler.
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
    if (!expanded || !apiKeyInput.trim()) return;
    await saveApiKey(expanded.id, apiKeyInput.trim());
    setApiKeyInput("");
    setApiKeySaved(true);
    setTimeout(() => setApiKeySaved(false), 2000);
  };

  const handleSelectModel = (value: string) => {
    if (!expanded) return;
    setModelSelectOpen(false);
    void updateProviderConfig(expanded.id, { model: value === AUTO_MODEL_VALUE ? null : value });
  };

  const handleAddModelToCatalog = async () => {
    if (!expanded) return;
    const trimmed = newModelDraft.trim();
    if (!trimmed) return;
    const existingKnown =
      settings?.providers.find((p) => p.id === expanded.id)?.knownModels ?? expanded.knownModels ?? [];
    const knownModels = mergeKnownModels(existingKnown, [trimmed]);
    const patch: { knownModels: string[]; model?: string } = { knownModels };
    if (!expanded.model) {
      patch.model = trimmed;
    }
    await updateProviderConfig(expanded.id, patch);
    setNewModelDraft("");
  };

  const handleLoadModels = async () => {
    if (!expanded) return;
    setModelsLoading(true);
    setModelsError(null);
    try {
      const fetched = await loadModels(expanded.id);
      const existingKnown =
        settings?.providers.find((p) => p.id === expanded.id)?.knownModels ?? expanded.knownModels ?? [];
      await updateProviderConfig(expanded.id, {
        knownModels: mergeKnownModels(
          existingKnown,
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
    if (!expanded) return;
    setTesting(true);
    setTestResult(null);
    try {
      const reply = await testConnection(expanded.id);
      setTestResult({ ok: true, message: reply });
    } catch (e) {
      setTestResult({ ok: false, message: toMessage(e) });
    } finally {
      setTesting(false);
    }
  };

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

  const isConfigured = (providerId: string) => hasApiKeyMap[providerId] === true;

  return (
    <div className="settings-sections llm-tab">
      <div className="llm-provider-list">
        {providers.map((provider) => {
          const isOpen = provider.id === expandedId;
          const isActive = provider.id === activeId;
          const configured = isConfigured(provider.id);
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
                <div className="settings-row llm-provider-detail">
                  {isActive ? (
                    <p className="settings-hint llm-inline-success settings-hint-compact">
                      Этот провайдер используется для чата.
                    </p>
                  ) : (
                    <div className="settings-actions">
                      <button
                        type="button"
                        className="settings-btn"
                        disabled={busy}
                        onClick={() => void selectActiveProvider(provider.id)}
                      >
                        Использовать для чата
                      </button>
                    </div>
                  )}

                  <label className="clone-modal-field">
                    <span className="clone-modal-label">Base URL</span>
                    <input
                      className="clone-modal-input"
                      type="text"
                      value={baseUrlDraft}
                      disabled={busy || provider.isSystem}
                      onChange={(event) => setBaseUrlDraft(event.target.value)}
                      onBlur={() => {
                        if (provider.isSystem) return;
                        void updateProviderConfig(provider.id, { baseUrl: baseUrlDraft.trim() || null });
                      }}
                    />
                  </label>
                  <LlmEndpointPreview
                    baseUrl={provider.isSystem ? provider.baseUrl : baseUrlDraft.trim() || provider.baseUrl}
                  />
                  {provider.isSystem ? (
                    <p className="settings-hint settings-hint-compact">
                      Встроенный провайдер — base URL задаётся сборкой приложения.
                    </p>
                  ) : null}

                  <div className="clone-modal-field">
                    <span className="clone-modal-label" id="llm-model-label">
                      Активная модель
                    </span>
                    <div className="llm-model-row">
                      <div className="clone-select" ref={modelSelectRef}>
                        <button
                          type="button"
                          className={`clone-select-trigger${modelSelectOpen ? " is-open" : ""}`}
                          aria-haspopup="listbox"
                          aria-expanded={modelSelectOpen}
                          aria-labelledby="llm-model-label"
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
                            <button
                              type="button"
                              role="option"
                              aria-selected={!provider.model}
                              className={`clone-select-option${!provider.model ? " is-active" : ""}`}
                              onClick={() => handleSelectModel(AUTO_MODEL_VALUE)}
                            >
                              <span className="clone-select-path">{AUTO_MODEL_LABEL}</span>
                            </button>
                            {provider.model && !provider.knownModels.includes(provider.model) ? (
                              <button
                                type="button"
                                role="option"
                                aria-selected
                                className="clone-select-option is-active"
                                onClick={() => handleSelectModel(provider.model as string)}
                              >
                                <span className="clone-select-path">{provider.model}</span>
                              </button>
                            ) : null}
                            {provider.knownModels.map((id) => (
                              <button
                                key={id}
                                type="button"
                                role="option"
                                aria-selected={id === provider.model}
                                className={`clone-select-option${id === provider.model ? " is-active" : ""}`}
                                onClick={() => handleSelectModel(id)}
                              >
                                <span className="clone-select-path">{id}</span>
                              </button>
                            ))}
                          </div>
                        ) : null}
                      </div>
                      <button
                        type="button"
                        className="llm-model-refresh"
                        disabled={modelsLoading || busy}
                        title={modelsLoading ? "Загрузка списка моделей…" : "Загрузить модели с API в каталог"}
                        aria-label={modelsLoading ? "Загрузка списка моделей…" : "Загрузить модели с API в каталог"}
                        onClick={() => void handleLoadModels()}
                      >
                        <RefreshCw size={15} className={modelsLoading ? "spin" : ""} aria-hidden />
                      </button>
                    </div>
                  </div>

                  <div className="clone-modal-field">
                    <span className="clone-modal-label" id="llm-model-catalog-label">
                      Каталог моделей
                    </span>
                    <div className="llm-model-add-row">
                      <input
                        className="clone-modal-input"
                        type="text"
                        placeholder={CUSTOM_MODEL_PLACEHOLDER}
                        value={newModelDraft}
                        disabled={busy}
                        aria-labelledby="llm-model-catalog-label"
                        onChange={(event) => setNewModelDraft(event.target.value)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") {
                            event.preventDefault();
                            void handleAddModelToCatalog();
                          }
                        }}
                      />
                      <button
                        type="button"
                        className="settings-btn primary llm-model-add-btn"
                        disabled={busy || !newModelDraft.trim()}
                        onClick={() => void handleAddModelToCatalog()}
                      >
                        Добавить
                      </button>
                    </div>
                    {provider.knownModels.length > 0 ? (
                      <ul className="llm-model-catalog" aria-labelledby="llm-model-catalog-label">
                        {provider.knownModels.map((id) => {
                          const isActive = id === provider.model;
                          return (
                            <li key={id} className="llm-model-catalog-row">
                              <button
                                type="button"
                                className={`llm-model-catalog-item${isActive ? " is-active" : ""}`}
                                aria-pressed={isActive}
                                disabled={busy}
                                onClick={() => void handleSelectModel(id)}
                              >
                                <span className="llm-model-catalog-id">{id}</span>
                                {isActive ? (
                                  <span className="llm-model-catalog-badge">активна</span>
                                ) : null}
                              </button>
                            </li>
                          );
                        })}
                      </ul>
                    ) : (
                      <p className="settings-hint settings-hint-compact llm-model-catalog-empty">
                        Каталог пуст — добавьте модель выше или загрузите с API.
                      </p>
                    )}
                    <p className="settings-hint settings-hint-compact">{CUSTOM_MODEL_HINT}</p>
                  </div>
                  {modelsError ? <p className="settings-hint llm-inline-error">{modelsError}</p> : null}

                  <div className="llm-cert-section">
                    <button
                      type="button"
                      className="llm-cert-toggle"
                      aria-expanded={certOpen}
                      onClick={() => setCertOpen((v) => !v)}
                    >
                      {certOpen ? (
                        <ChevronDown className="llm-provider-chevron" size={14} aria-hidden />
                      ) : (
                        <ChevronRight className="llm-provider-chevron" size={14} aria-hidden />
                      )}
                      <span>Доверенный сертификат{provider.isSystem ? " (переопределение)" : ""}</span>
                    </button>

                    {certOpen ? (
                      <>
                        <label className="clone-modal-field">
                          <textarea
                            className="llm-cert-textarea"
                            placeholder={
                              provider.isSystem
                                ? "Встроенный сертификат: " +
                                  (provider.trustedCertPem ? "задан" : "не задан") +
                                  ". Вставьте свой, чтобы переопределить."
                                : "Вставьте сертификат в формате PEM, если эндпоинту требуется доверие к своему CA."
                            }
                            value={certDraft}
                            disabled={busy}
                            onChange={(event) => setCertDraft(event.target.value)}
                          />
                        </label>
                        <div className="settings-actions">
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
                              Сбросить к встроенному
                            </button>
                          ) : null}
                        </div>
                      </>
                    ) : null}
                  </div>

                  <label className="clone-modal-field">
                    <span className="clone-modal-label">API ключ</span>
                    <div className="llm-api-key-row">
                      <input
                        className="clone-modal-input"
                        type="password"
                        placeholder={hasApiKeyMap[provider.id] ? "Ключ сохранён — введите новый, чтобы заменить" : "sk-..."}
                        value={apiKeyInput}
                        disabled={busy}
                        onChange={(event) => setApiKeyInput(event.target.value)}
                      />
                      <button
                        type="button"
                        className="llm-api-key-save"
                        disabled={busy || !apiKeyInput.trim()}
                        title={apiKeySaved ? "Сохранено!" : "Сохранить ключ"}
                        aria-label={apiKeySaved ? "Сохранено!" : "Сохранить ключ"}
                        onClick={() => void handleSaveApiKey()}
                      >
                        {apiKeySaved ? <Check size={15} aria-hidden /> : <Save size={15} aria-hidden />}
                      </button>
                    </div>
                  </label>

                  <div className="settings-actions">
                    <button
                      type="button"
                      className="settings-btn primary"
                      disabled={testing}
                      onClick={() => void handleTestConnection()}
                    >
                      {testing ? "Проверка…" : "Проверить соединение"}
                    </button>
                    {!testing && testResult ? (
                      <span className="llm-test-result-icon" title={testResult.message}>
                        {testResult.ok ? (
                          <CheckCircle2 className="ok" size={18} aria-label={testResult.message} />
                        ) : (
                          <XCircle className="error" size={18} aria-label={testResult.message} />
                        )}
                      </span>
                    ) : null}
                    {!provider.isSystem ? (
                      <button
                        type="button"
                        className="settings-link-btn danger"
                        onClick={() => {
                          setExpandedId(null);
                          void removeProvider(provider.id);
                        }}
                      >
                        Удалить провайдера
                      </button>
                    ) : null}
                  </div>
                </div>
              ) : null}
            </div>
          );
        })}
        {providers.length === 0 ? (
          <p className="settings-hint settings-hint-compact">
            Провайдеры не настроены.
          </p>
        ) : null}
      </div>

      <div className="settings-card llm-add-provider">
        <div className="settings-section-title">Добавить провайдера</div>
        <p className="settings-hint settings-hint-compact">
          Укажите название и корень OpenAI-совместимого API (например{" "}
          <span className="llm-endpoint-suffix">https://openrouter.ai/api/v1</span>) — модель, сертификат и ключ
          настраиваются после добавления.
        </p>
        <label className="clone-modal-field">
          <span className="clone-modal-label">Название</span>
          <input
            className="clone-modal-input"
            type="text"
            placeholder="Мой провайдер"
            value={newProviderLabel}
            onChange={(event) => setNewProviderLabel(event.target.value)}
          />
        </label>
        <label className="clone-modal-field">
          <span className="clone-modal-label">Base URL</span>
          <input
            className="clone-modal-input"
            type="text"
            placeholder="https://api.openai.com/v1"
            value={newProviderBaseUrl}
            onChange={(event) => setNewProviderBaseUrl(event.target.value)}
          />
        </label>
        <LlmEndpointPreview baseUrl={newProviderBaseUrl} />
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
          Для AlfaGen — скользящее окно EVC. Выключите, чтобы скрыть чип и не
          записывать расход токенов.
        </p>
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
