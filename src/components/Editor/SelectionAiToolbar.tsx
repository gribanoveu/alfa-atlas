import { ChevronDown, ChevronUp, Loader2, Sparkles, Wand2 } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import type { SelectionAiAction } from "../../lib/selectionAiPrompts";
import { SELECTION_AI_MAX_CHARS } from "../../lib/selectionAiPrompts";
import type { SelectionAiUiState } from "../../hooks/useMonacoSelectionAi";

type SelectionAiToolbarProps = {
  state: SelectionAiUiState;
  onAction: (action: SelectionAiAction, customPrompt?: string) => void;
  onToggleCustom: (open: boolean) => void;
  onAddToChat: () => void;
  onToggleMore: (open: boolean) => void;
};

const ACTIONS: { id: Exclude<SelectionAiAction, "custom">; label: string }[] = [
  { id: "rewrite", label: "Переписать" },
  { id: "shorten", label: "Сократить" },
  { id: "expand", label: "Расширить" },
  { id: "fix", label: "Исправить" },
];

const EDGE = 8;

export function SelectionAiToolbar({
  state,
  onAction,
  onToggleCustom,
  onAddToChat,
  onToggleMore,
}: SelectionAiToolbarProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: 320, height: 36 });
  const [customPrompt, setCustomPrompt] = useState("");

  const visible = state.phase === "toolbar";

  // Remeasure when the custom-prompt row or the «Больше» actions row
  // opens/closes — that changes width a lot, and centering via translate(-50%)
  // used to shove it past the left edge.
  useLayoutEffect(() => {
    if (!visible) return;
    const el = rootRef.current;
    if (!el) return;
    const width = el.offsetWidth;
    const height = el.offsetHeight;
    if (width > 0 && height > 0) {
      setSize({ width, height });
    }
  }, [visible, state.customPromptOpen, state.moreExpanded, state.phase, state.tooLong]);

  if (!visible || !state.position) return null;

  const busy = state.phase === "loading";
  const disabled = busy || !state.llmReady || state.tooLong;

  const disabledTitle = !state.llmReady
    ? "Настройте LLM в настройках"
    : state.tooLong
      ? `Слишком длинное выделение (макс. ${SELECTION_AI_MAX_CHARS} символов)`
      : undefined;

  const overlay = rootRef.current?.closest(".selection-ai-overlay");
  const overlayW =
    overlay instanceof HTMLElement ? overlay.clientWidth : Number.POSITIVE_INFINITY;
  const overlayH =
    overlay instanceof HTMLElement ? overlay.clientHeight : Number.POSITIVE_INFINITY;

  let left = state.position.left - size.width / 2;
  let top =
    state.position.toolbarPlacement === "above"
      ? state.position.top - size.height
      : state.position.top;

  if (Number.isFinite(overlayW)) {
    left = Math.min(overlayW - size.width - EDGE, Math.max(EDGE, left));
  }
  if (Number.isFinite(overlayH)) {
    top = Math.min(overlayH - size.height - EDGE, Math.max(EDGE, top));
  }

  return (
    <div
      ref={rootRef}
      className="selection-ai-toolbar"
      style={{ top, left }}
      role="toolbar"
      aria-label="AI-действия для выделения"
    >
      <span className="selection-ai-toolbar-icon" aria-hidden>
        {busy ? <Loader2 size={14} className="selection-ai-spin" /> : <Sparkles size={14} />}
      </span>
      <button
        type="button"
        className="selection-ai-btn"
        disabled={busy}
        title="Отправить выделенное в чат ассистента"
        onClick={onAddToChat}
      >
        Добавить в чат
      </button>
      <span className="selection-ai-sep" role="separator" aria-orientation="vertical" />
      <button
        type="button"
        className={
          state.moreExpanded
            ? "selection-ai-btn selection-ai-btn-active"
            : "selection-ai-btn"
        }
        title={state.moreExpanded ? "Скрыть дополнительные действия" : "Показать все действия"}
        aria-expanded={state.moreExpanded}
        onClick={() => onToggleMore(!state.moreExpanded)}
      >
        Больше
        {state.moreExpanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
      </button>
      {state.moreExpanded && !busy ? (
        <div className="selection-ai-more-row">
          {ACTIONS.map((action) => (
            <button
              key={action.id}
              type="button"
              className={
                state.activeAction === action.id && busy
                  ? "selection-ai-btn selection-ai-btn-active"
                  : "selection-ai-btn"
              }
              disabled={disabled}
              title={disabledTitle ?? action.label}
              onClick={() => onAction(action.id)}
            >
              {action.label}
            </button>
          ))}
          <button
            type="button"
            className={
              state.customPromptOpen
                ? "selection-ai-btn selection-ai-btn-active"
                : "selection-ai-btn"
            }
            disabled={disabled}
            title={disabledTitle ?? "Свой запрос"}
            aria-label="Свой запрос"
            onClick={() => onToggleCustom(!state.customPromptOpen)}
          >
            <Wand2 size={14} />
          </button>
        </div>
      ) : null}
      {state.moreExpanded && !busy && state.customPromptOpen ? (
        <form
          className="selection-ai-custom"
          onSubmit={(event) => {
            event.preventDefault();
            const prompt = customPrompt.trim();
            if (!prompt) return;
            onAction("custom", prompt);
          }}
        >
          <input
            className="selection-ai-custom-input"
            type="text"
            value={customPrompt}
            onChange={(event) => setCustomPrompt(event.target.value)}
            placeholder="Свой запрос…"
            autoFocus
            disabled={disabled}
          />
          <button
            type="submit"
            className="selection-ai-btn selection-ai-btn-primary"
            disabled={disabled || !customPrompt.trim()}
          >
            Отправить
          </button>
        </form>
      ) : null}
      {state.tooLong ? (
        <span className="selection-ai-hint">Слишком длинное выделение</span>
      ) : null}
    </div>
  );
}
