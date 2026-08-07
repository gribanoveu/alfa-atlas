import { AlertCircle, CheckCircle2, ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useLlmSetup } from "../../hooks/useLlmSetup";
import { AUTO_MODEL_LABEL, AUTO_MODEL_VALUE } from "../../lib/assistantConfig";
import type { LlmModelInfo } from "../../lib/llm";
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

export function LlmTab() {
  const {
    settings,
    providers,
    hasApiKeyMap,
    busy,
    error,
    selectActiveProvider,
    updateProviderConfig,
    setDebugLogging,
    removeProvider,
    saveApiKey,
    loadModels,
    testConnection,
  } = useLlmSetup();

  const activeId = settings?.activeProviderId ?? providers[0]?.id ?? null;

  // Accordion: at most one provider's settings body is open at a time —
  // separate from `activeId` (which provider chat actually uses), so
  // reviewing/editing a provider's config no longer has the side effect of
  // switching the app to it.
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const expanded = providers.find((p) => p.id === expandedId) ?? null;

  const [baseUrlDraft, setBaseUrlDraft] = useState("");
  const [certDraft, setCertDraft] = useState("");
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [apiKeySaved, setApiKeySaved] = useState(false);
  const [models, setModels] = useState<LlmModelInfo[]>([]);
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
    setModels([]);
    setModelsError(null);
    setTestResult(null);
    setModelSelectOpen(false);
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
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setModelSelectOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
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

  const handleLoadModels = async () => {
    if (!expanded) return;
    setModelsLoading(true);
    setModelsError(null);
    try {
      setModels(await loadModels(expanded.id));
    } catch (e) {
      setModelsError(e instanceof Error ? e.message : String(e));
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
      setTestResult({ ok: false, message: e instanceof Error ? e.message : String(e) });
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
    <div className="llm-tab">
      <div className="settings-section-title">Провайдеры LLM</div>
      <p className="settings-lead">
        Провайдер языковой модели для будущего чата с ассистентом (отдельно от
        провайдера эмбеддингов). Встроенные провайдеры уже настроены — нужно
        только вставить API-ключ; можно также добавить свой.
      </p>

      <label className="credentials-checkbox-label">
        <input
          type="checkbox"
          className="credentials-checkbox"
          checked={settings?.debugLogging ?? false}
          disabled={busy || !settings}
          onChange={(event) => void setDebugLogging(event.target.checked)}
        />
        <span>Логировать запросы и ответы модели</span>
      </label>
      <p className="settings-hint" style={{ paddingLeft: 0 }}>
        Записывает каждый запрос и ответ (включая промежуточные шаги вызова инструментов) в{" "}
        <code>~/.atlas/logs/llm.jsonl</code> — полезно, чтобы разобраться в ошибке провайдера.
        Выключено по умолчанию: переписка может содержать содержимое документов.
      </p>

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
                    <p className="settings-hint llm-inline-success" style={{ paddingLeft: 0 }}>
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
                  {provider.isSystem ? (
                    <p className="settings-hint" style={{ paddingLeft: 0 }}>
                      Встроенный провайдер — base URL задаётся сборкой приложения.
                    </p>
                  ) : null}

                  <div className="clone-modal-field">
                    <span className="clone-modal-label" id="llm-model-label">
                      Модель
                    </span>
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
                          {provider.model && !models.some((m) => m.id === provider.model) ? (
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
                          {models.map((m) => (
                            <button
                              key={m.id}
                              type="button"
                              role="option"
                              aria-selected={m.id === provider.model}
                              className={`clone-select-option${m.id === provider.model ? " is-active" : ""}`}
                              onClick={() => handleSelectModel(m.id)}
                            >
                              <span className="clone-select-path">{m.id}</span>
                            </button>
                          ))}
                        </div>
                      ) : null}
                    </div>
                  </div>
                  <div className="settings-actions">
                    <button
                      type="button"
                      className="settings-btn"
                      disabled={modelsLoading}
                      onClick={() => void handleLoadModels()}
                    >
                      {modelsLoading ? "Загрузка списка моделей…" : "Обновить список моделей"}
                    </button>
                  </div>
                  {modelsError ? <p className="settings-hint llm-inline-error">{modelsError}</p> : null}

                  <label className="clone-modal-field">
                    <span className="clone-modal-label">
                      Доверенный сертификат{provider.isSystem ? " (переопределение)" : ""}
                    </span>
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
                      onClick={() => void updateProviderConfig(provider.id, { trustedCertPem: certDraft.trim() || null })}
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

                  <label className="clone-modal-field">
                    <span className="clone-modal-label">API ключ</span>
                    <input
                      className="clone-modal-input"
                      type="password"
                      placeholder={hasApiKeyMap[provider.id] ? "Ключ сохранён — введите новый, чтобы заменить" : "sk-..."}
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
                  </div>

                  <div className="settings-actions">
                    <button
                      type="button"
                      className="settings-btn primary"
                      disabled={testing}
                      onClick={() => void handleTestConnection()}
                    >
                      {testing ? "Проверка…" : "Проверить соединение"}
                    </button>
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
                  {testResult ? (
                    <p className={`settings-hint llm-inline-${testResult.ok ? "success" : "error"}`}>
                      {testResult.message}
                    </p>
                  ) : null}
                </div>
              ) : null}
            </div>
          );
        })}
        {providers.length === 0 ? (
          <p className="settings-hint" style={{ paddingLeft: 0 }}>
            Провайдеры не настроены.
          </p>
        ) : null}
      </div>

      <hr className="credentials-divider" />

      <div className="settings-row llm-add-provider">
        <div className="settings-section-title">Добавить провайдера</div>
        <p className="settings-hint" style={{ paddingLeft: 0 }}>
          Укажите название и адрес OpenAI-совместимого API — модель, сертификат и ключ
          настраиваются после добавления, в развёрнутой карточке провайдера.
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

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
