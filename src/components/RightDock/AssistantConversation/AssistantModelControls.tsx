import { useEffect, useRef, useState } from "react";
import { ChevronUp, FileText, FolderGit2, type LucideIcon } from "lucide-react";
import {
  AUTO_MODEL_LABEL,
  AUTO_MODEL_VALUE,
  CHAT_MODEL_CATALOG_EMPTY_HINT,
  CONTEXT_NEAR_LIMIT_RATIO,
} from "../../../lib/assistantConfig";
import type { AiAccessMode, ConversationMode } from "../../../lib/aiTools";
import type { LlmProviderConfig, ResolvedLlmProvider } from "../../../lib/llm";
import { trackMetric } from "../../../lib/metrics";
import { METRICS } from "../../../data/metricsCatalog";

const ACCESS_MODE_OPTIONS: { value: AiAccessMode; label: string; Icon: LucideIcon }[] = [
  { value: "docsOnly", label: "Документация", Icon: FileText },
  { value: "fullRepo", label: "Весь репозиторий", Icon: FolderGit2 },
];

export const CHAT_MODE_OPTIONS = [
  {
    value: "agent",
    label: "Агент",
    title: "изучу и сделаю — исследование плюс правки",
    greeting: "Расскажите, что нужно сделать — изучу проект и внесу правки в документацию.",
  },
  {
    value: "plan",
    label: "План",
    title: "внимательно изучу и предложу план — без правок",
    greeting:
      "Опишите задачу — внимательно изучу все варианты и нюансы, прежде чем предложить план. Файлы не меняю.",
  },
  {
    value: "question",
    label: "Вопрос",
    title: "быстро отвечу на точечный вопрос — без анализа",
    greeting: "Спросите о проекте — быстро разберусь и отвечу, не меняя файлы.",
  },
] as const;

function AccessModeToggle({
  mode,
  onChange,
  disabled,
}: {
  mode: AiAccessMode;
  onChange: (mode: AiAccessMode) => void;
  disabled: boolean;
}) {
  return (
    <div className="assistant-access-toggle" role="radiogroup" aria-label="Область доступа AI">
      {ACCESS_MODE_OPTIONS.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={mode === option.value}
          aria-label={option.label}
          title={option.label}
          className={`assistant-access-btn${mode === option.value ? " active" : ""}`}
          disabled={disabled}
          onClick={() => onChange(option.value)}
        >
          <option.Icon size={12} strokeWidth={1.75} aria-hidden />
        </button>
      ))}
    </div>
  );
}

function ChatModeSelect({
  mode,
  onChange,
  disabled,
}: {
  mode: ConversationMode;
  onChange: (mode: ConversationMode) => void;
  disabled: boolean;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const active = CHAT_MODE_OPTIONS.find((option) => option.value === mode);

  return (
    <div className="assistant-mode-select" ref={ref}>
      <button
        type="button"
        className={`assistant-mode-trigger${open ? " is-open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        title={active?.title ?? "Режим ассистента"}
        disabled={disabled}
        onClick={() => setOpen((value) => !value)}
      >
        <span>{active?.label ?? ""}</span>
        <ChevronUp className="assistant-mode-chevron" size={12} aria-hidden />
      </button>
      {open ? (
        <div className="assistant-mode-menu" role="listbox">
          {CHAT_MODE_OPTIONS.map((option) => (
            <button
              key={option.value}
              type="button"
              role="option"
              aria-selected={mode === option.value}
              title={option.title}
              className={`assistant-mode-option${mode === option.value ? " is-active" : ""}`}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
              }}
            >
              <span>{option.label}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function trimTrailingZero(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

function formatTokenCount(value: number): string {
  if (value >= 1_000_000) return `${trimTrailingZero(value / 1_000_000)}M`;
  if (value >= 1_000) return `${trimTrailingZero(value / 1_000)}K`;
  return String(value);
}

function contextUsageTitle(
  contextTokens: number,
  contextLimit: number,
  lastRequestTokens: number | null,
  sending: boolean,
): string {
  const ru = (value: number) => value.toLocaleString("ru-RU");
  const head = sending
    ? `Запрос сейчас: ~${ru(contextTokens)} из ${ru(contextLimit)} токенов`
    : `Следующий запрос: ~${ru(contextTokens)} из ${ru(contextLimit)} токенов`;
  return lastRequestTokens === null
    ? head
    : `${head}\nПоследний отправленный запрос: ${ru(lastRequestTokens)}`;
}

const CONTEXT_RING_RADIUS = 8;
const CONTEXT_RING_CIRCUMFERENCE = 2 * Math.PI * CONTEXT_RING_RADIUS;

type AssistantModelControlsProps = {
  providerId: string | null;
  activeProvider: ResolvedLlmProvider | null;
  sending: boolean;
  accessMode: AiAccessMode;
  accessModeBusy: boolean;
  conversationMode: ConversationMode;
  contextTokens: number;
  lastRequestTokens: number | null;
  onConversationModeChange: (mode: ConversationMode) => void;
  onAccessModeChange: (mode: AiAccessMode) => void;
  updateProviderConfig: (
    providerId: string,
    patch: Partial<Omit<LlmProviderConfig, "id">>,
  ) => Promise<void>;
  refreshLlmSetup: () => Promise<void>;
};

export function AssistantModelControls({
  providerId,
  activeProvider,
  sending,
  accessMode,
  accessModeBusy,
  conversationMode,
  contextTokens,
  lastRequestTokens,
  onConversationModeChange,
  onAccessModeChange,
  updateProviderConfig,
  refreshLlmSetup,
}: AssistantModelControlsProps) {
  const [modelSelectOpen, setModelSelectOpen] = useState(false);
  const modelSelectRef = useRef<HTMLDivElement>(null);
  const contextLimit = activeProvider?.limit?.context ?? null;
  const contextUsageRatio = contextLimit
    ? Math.min(1, contextTokens / contextLimit)
    : null;
  const catalogModels = activeProvider?.knownModels ?? [];

  useEffect(() => {
    setModelSelectOpen(false);
  }, [providerId]);

  useEffect(() => {
    if (!modelSelectOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!modelSelectRef.current?.contains(event.target as Node)) {
        setModelSelectOpen(false);
      }
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

  const handleToggleModelSelect = () => {
    setModelSelectOpen((open) => {
      const next = !open;
      if (next) void refreshLlmSetup();
      return next;
    });
  };

  const handleSelectModel = (value: string) => {
    if (!providerId) return;
    setModelSelectOpen(false);
    void updateProviderConfig(providerId, {
      model: value === AUTO_MODEL_VALUE ? null : value,
    });
  };

  return (
    <div className="assistant-model-bar">
      <ChatModeSelect
        mode={conversationMode}
        onChange={(next) => {
          void trackMetric(METRICS.ASSISTANT.SWITCH_CONVERSATION_MODE, undefined, {
            label: "user",
            property: next,
          });
          onConversationModeChange(next);
        }}
        disabled={sending}
      />
      <AccessModeToggle
        mode={accessMode}
        onChange={(next) => {
          void trackMetric(METRICS.ASSISTANT.SWITCH_ACCESS_MODE, undefined, {
            label: "user",
            property: next,
          });
          onAccessModeChange(next);
        }}
        disabled={sending || accessModeBusy}
      />
      <div className="clone-select assistant-model-select" ref={modelSelectRef}>
        <button
          type="button"
          className={`clone-select-trigger${modelSelectOpen ? " is-open" : ""}`}
          aria-haspopup="listbox"
          aria-expanded={modelSelectOpen}
          disabled={sending}
          onClick={handleToggleModelSelect}
        >
          <span className="clone-select-value">
            <span className="clone-select-path">
              {activeProvider?.model ?? AUTO_MODEL_LABEL}
            </span>
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
              aria-selected={!activeProvider?.model}
              className={`clone-select-option${!activeProvider?.model ? " is-active" : ""}`}
              onClick={() => handleSelectModel(AUTO_MODEL_VALUE)}
            >
              <span className="clone-select-path">{AUTO_MODEL_LABEL}</span>
            </button>
            {catalogModels.length === 0 && !activeProvider?.model ? (
              <div className="clone-select-option is-disabled" aria-disabled>
                <span className="clone-select-path">
                  {CHAT_MODEL_CATALOG_EMPTY_HINT}
                </span>
              </div>
            ) : null}
            {activeProvider?.model &&
            !catalogModels.includes(activeProvider.model) ? (
              <button
                type="button"
                role="option"
                aria-selected
                className="clone-select-option is-active"
                onClick={() => handleSelectModel(activeProvider.model as string)}
              >
                <span className="clone-select-path">{activeProvider.model}</span>
              </button>
            ) : null}
            {catalogModels.map((id) => (
              <button
                key={id}
                type="button"
                role="option"
                aria-selected={id === activeProvider?.model}
                className={`clone-select-option${id === activeProvider?.model ? " is-active" : ""}`}
                onClick={() => handleSelectModel(id)}
              >
                <span className="clone-select-path">{id}</span>
              </button>
            ))}
          </div>
        ) : null}
      </div>
      {contextLimit !== null ? (
        <div
          className={`assistant-context-bar${
            contextUsageRatio !== null &&
            contextUsageRatio >= CONTEXT_NEAR_LIMIT_RATIO
              ? " near-limit"
              : ""
          }`}
          title={contextUsageTitle(
            contextTokens,
            contextLimit,
            lastRequestTokens,
            sending,
          )}
        >
          <svg
            className="assistant-context-ring"
            width="20"
            height="20"
            viewBox="0 0 20 20"
            aria-hidden
          >
            <circle
              className="assistant-context-ring-track"
              cx="10"
              cy="10"
              r={CONTEXT_RING_RADIUS}
            />
            <circle
              className="assistant-context-ring-fill"
              cx="10"
              cy="10"
              r={CONTEXT_RING_RADIUS}
              strokeDasharray={CONTEXT_RING_CIRCUMFERENCE}
              strokeDashoffset={
                CONTEXT_RING_CIRCUMFERENCE * (1 - (contextUsageRatio ?? 0))
              }
            />
          </svg>
          <span className="assistant-context-bar-label">
            {formatTokenCount(contextTokens)} / {formatTokenCount(contextLimit)}
          </span>
        </div>
      ) : null}
    </div>
  );
}
