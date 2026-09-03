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
import { CERT_PLACEHOLDER } from "./certField";
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

/** The vocabulary OpenAI-compatible gateways actually accept for
 * `reasoning_effort`. Kept as a UI-side list rather than a Rust enum on
 * purpose: the backend stores a free string (see
 * `domain::llm::LlmProviderConfig::reasoning_effort`), so a gateway with
 * its own spelling stays reachable by hand-editing `settings.json` — and a
 * value saved that way is added to this list on the fly below rather than
 * silently replaced the first time the dropdown is opened. */
const REASONING_EFFORT_OPTIONS: { value: string; label: string }[] = [
  { value: "", label: "не отправлять" },
  { value: "minimal", label: "minimal" },
  { value: "low", label: "low" },
  { value: "medium", label: "medium" },
  { value: "high", label: "high" },
];

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
  // Только пользовательский слой, не разрешённое значение: вшитый в сборку
  // PEM в редактируемом поле неотличим от своего, а сохранение такого поля
  // закрепило бы дефолт сборки как override — и обновление манифеста до
  // этого пользователя больше не доехало бы.
  const [certDraft, setCertDraft] = useState(provider.trustedCertOverride ?? "");
  const [contextDraft, setContextDraft] = useState(String(effectiveTokenLimit(provider).context));
  const [outputDraft, setOutputDraft] = useState(String(effectiveTokenLimit(provider).output));
  const [temperatureDraft, setTemperatureDraft] = useState(provider.temperature?.toString() ?? "");
  const [maxTokensDraft, setMaxTokensDraft] = useState(provider.maxTokens?.toString() ?? "");
  const [effortSelectOpen, setEffortSelectOpen] = useState(false);
  const effortSelectRef = useRef<HTMLDivElement>(null);
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
    setCertDraft(provider.trustedCertOverride ?? "");
  }, [provider.id, provider.trustedCertOverride]);

  useEffect(() => {
    const limit = effectiveTokenLimit(provider);
    setContextDraft(String(limit.context));
    setOutputDraft(String(limit.output));
  }, [provider.id, provider.limit?.context, provider.limit?.output]);

  useEffect(() => {
    setTemperatureDraft(provider.temperature?.toString() ?? "");
  }, [provider.id, provider.temperature]);

  useEffect(() => {
    setMaxTokensDraft(provider.maxTokens?.toString() ?? "");
  }, [provider.id, provider.maxTokens]);

  useEffect(() => {
    setEffortSelectOpen(false);
  }, [provider.id]);

  useEffect(() => {
    if (!effortSelectOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!effortSelectRef.current?.contains(event.target as Node)) setEffortSelectOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      setEffortSelectOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [effortSelectOpen]);

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

  /** Вызывается по потере фокуса и по Enter, а не только кнопкой: ключ,
   * набранный и оставленный в поле, иначе молча пропадал. Пустая строка
   * отсекается здесь же, поэтому уход с нетронутого поля ничего не пишет, а
   * клик по кнопке не сохраняет дважды — blur успевает очистить поле, и к
   * моменту клика кнопка уже неактивна. */
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

  /** An empty field means "send no `temperature` at all" — not zero, and not
   * the API's own default. Anything unparseable or out of range snaps back
   * to what is saved rather than writing a value the provider will reject. */
  const handleSaveTemperature = () => {
    const trimmed = temperatureDraft.trim();
    if (trimmed === "") {
      if (provider.temperature !== null) void updateProviderConfig(provider.id, { temperature: null });
      return;
    }
    const parsed = Number(trimmed.replace(",", "."));
    if (!Number.isFinite(parsed) || parsed < 0 || parsed > 2) {
      setTemperatureDraft(provider.temperature?.toString() ?? "");
      return;
    }
    if (parsed === provider.temperature) return;
    void updateProviderConfig(provider.id, { temperature: parsed });
  };

  /** Same "empty means send nothing" contract as the temperature field: an
   * absent `max_tokens` leaves the server's own default in place, which is
   * not the same as any number we could put here. */
  const handleSaveMaxTokens = () => {
    const trimmed = maxTokensDraft.trim();
    if (trimmed === "") {
      if (provider.maxTokens !== null) void updateProviderConfig(provider.id, { maxTokens: null });
      return;
    }
    const parsed = Number(trimmed);
    if (!Number.isInteger(parsed) || parsed <= 0) {
      setMaxTokensDraft(provider.maxTokens?.toString() ?? "");
      return;
    }
    if (parsed === provider.maxTokens) return;
    void updateProviderConfig(provider.id, { maxTokens: parsed });
  };

  /** The empty option is not "medium by default" — it keeps
   * `reasoning_effort` out of the request entirely, which is the only safe
   * choice for a gateway that doesn't implement the key: those generally
   * reject the whole request for carrying it rather than ignoring it. */
  const handleSelectReasoningEffort = (value: string) => {
    setEffortSelectOpen(false);
    const next = value === "" ? null : value;
    if (next === provider.reasoningEffort) return;
    void updateProviderConfig(provider.id, { reasoningEffort: next });
  };

  /** The fixed vocabulary, plus whatever this provider is actually set to
   * if that came from outside the list (a hand-edited `settings.json`, a
   * gateway with its own spelling) — so opening the dropdown can never be
   * the thing that loses a working value. */
  const reasoningEffortOptions =
    provider.reasoningEffort && !REASONING_EFFORT_OPTIONS.some((o) => o.value === provider.reasoningEffort)
      ? [...REASONING_EFFORT_OPTIONS, { value: provider.reasoningEffort, label: provider.reasoningEffort }]
      : REASONING_EFFORT_OPTIONS;

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
              onBlur={() => void handleSaveApiKey()}
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

      <section className="llm-detail-group">
        <h4 className="llm-detail-group-title">Генерация</h4>
        <label className="llm-field">
          <span className="llm-field-label">Температура</span>
          <input
            className="clone-modal-input"
            type="number"
            min={0}
            max={2}
            step={0.1}
            placeholder="не отправлять"
            value={temperatureDraft}
            disabled={busy}
            onChange={(event) => setTemperatureDraft(event.target.value)}
            onBlur={handleSaveTemperature}
          />
        </label>
        <p className="settings-hint settings-hint-compact">
          Ниже — предсказуемее и суше, выше — разнообразнее. Для документации подходит 0.2–0.4. Пустое
          поле означает, что параметр не отправляется вовсе: часть моделей с рассуждением отвергает
          запрос, в котором он есть.
        </p>

        <label className="llm-field">
          <span className="llm-field-label">Лимит ответа</span>
          <input
            className="clone-modal-input"
            type="number"
            min={1}
            step={256}
            placeholder="не отправлять"
            value={maxTokensDraft}
            disabled={busy}
            onChange={(event) => setMaxTokensDraft(event.target.value)}
            onBlur={handleSaveMaxTokens}
          />
        </label>
        <p className="settings-hint settings-hint-compact">
          Уходит в запрос как <code>max_tokens</code> — в отличие от «Макс. ответ» в блоке
          «Контекст», где число только информационное и нужно счётчику контекста. Ориентиры:
          4000–8000 хватает на обычный ответ с вызовами инструментов, 16000 и выше — если ассистент
          целиком переписывает большой <code>.adoc</code>. У модели с рассуждением в этот лимит
          входят и сами размышления, так что запас нужен больше, иначе ответ оборвётся на полуслове.
          Пустое поле — параметр не отправляется, и длину ограничивает только сам провайдер.
        </p>

        <div className="llm-field">
          <span className="llm-field-label" id="reasoning-effort-label">
            Усилие рассуждения
          </span>
          <div className="clone-select llm-model-select" ref={effortSelectRef}>
            <button
              type="button"
              className={`clone-select-trigger${effortSelectOpen ? " is-open" : ""}`}
              aria-haspopup="listbox"
              aria-expanded={effortSelectOpen}
              aria-labelledby="reasoning-effort-label"
              disabled={busy}
              onClick={() => setEffortSelectOpen((open) => !open)}
            >
              <span className="clone-select-value">
                <span className="clone-select-path">
                  {reasoningEffortOptions.find((o) => o.value === (provider.reasoningEffort ?? ""))
                    ?.label ?? "не отправлять"}
                </span>
              </span>
              <span className="clone-select-chevron" aria-hidden>
                ▾
              </span>
            </button>
            {effortSelectOpen ? (
              <div className="clone-select-menu" role="listbox">
                {reasoningEffortOptions.map((option) => {
                  const active = option.value === (provider.reasoningEffort ?? "");
                  return (
                    <button
                      key={option.value || "none"}
                      type="button"
                      role="option"
                      aria-selected={active}
                      className={`clone-select-option${active ? " is-active" : ""}`}
                      onClick={() => handleSelectReasoningEffort(option.value)}
                    >
                      <span className="clone-select-path">{option.label}</span>
                    </button>
                  );
                })}
              </div>
            ) : null}
          </div>
        </div>
        <p className="settings-hint settings-hint-compact">
          <code>reasoning_effort</code> для моделей с рассуждением: чем ниже, тем короче модель
          думает перед ответом и тем быстрее отвечает. «Не отправлять» вообще убирает параметр из
          запроса — шлюз, который его не понимает, обычно отвергает весь запрос целиком, а не
          игнорирует ключ, поэтому это и есть значение по умолчанию.
        </p>
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
                Доверенный сертификат{provider.hasBundledCert ? " (переопределение)" : ""}
              </span>
              <textarea
                className="llm-textarea"
                rows={4}
                placeholder={CERT_PLACEHOLDER}
                value={certDraft}
                disabled={busy}
                onChange={(event) => setCertDraft(event.target.value)}
              />
              <p className="settings-hint settings-hint-compact">
                {provider.hasBundledCert
                  ? "Сертификат задан сборкой приложения — поле пустое, пока вы его не переопределили."
                  : "PEM сертификата CA, если эндпоинт не доверен системой."}
              </p>
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
                {/* Сбрасывать есть что только при своём сертификате — он и
                    исчезает, возвращая сертификат сборки, если тот есть. */}
                {provider.trustedCertOverride ? (
                  <button
                    type="button"
                    className="settings-btn"
                    disabled={busy}
                    onClick={() => void updateProviderConfig(provider.id, { trustedCertPem: null })}
                  >
                    {provider.hasBundledCert ? "Вернуть сертификат сборки" : "Сбросить"}
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
    setRateLimitOffHoursEnforced,
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
          Для AlfaGen — скользящее окно EVC: токены запроса, токены ответа и число
          обращений считаются отдельно, отказ приходит по любому из трёх.
          Выключите, чтобы скрыть чип и не записывать расход.
        </p>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={settings?.rateLimitOffHoursEnforced ?? false}
            disabled={busy || !settings || !(settings?.rateLimitEnabled ?? true)}
            onChange={(event) => void setRateLimitOffHoursEnforced(event.target.checked)}
          />
          <span>Считать и в нерабочее время</span>
        </label>
        <p className="settings-hint">
          Вне будних 9:00–19:00 сервер лимиты не проверяет, поэтому по умолчанию
          чип в это время показывает «без лимита». Включите, чтобы расход
          считался круглосуточно — пригодится, если график лимитов изменился.
        </p>
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
