import { Check, GripVertical, Loader2, RotateCcw, X } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import Draggable from "react-draggable";
import type { SelectionAiUiState } from "../../hooks/useMonacoSelectionAi";

type SelectionAiPreviewProps = {
  state: SelectionAiUiState;
  onAccept: () => void;
  onReject: () => void;
  onRetry: () => void;
};

const EDGE = 8;

export function SelectionAiPreview({
  state,
  onAccept,
  onReject,
  onRetry,
}: SelectionAiPreviewProps) {
  const nodeRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: 360, height: 180 });

  const open =
    state.phase === "loading" ||
    state.phase === "preview" ||
    state.phase === "error";

  // Keep drag offset across loading↔preview; only reset when the text changes
  // or the card is dismissed.
  const dragKey = !open || !state.position
    ? "hidden"
    : `card:${state.selectedText}`;

  useLayoutEffect(() => {
    const el = nodeRef.current;
    if (!el) return;
    const width = el.offsetWidth;
    const height = el.offsetHeight;
    if (width > 0 && height > 0) {
      setSize({ width, height });
    }
  }, [dragKey, state.phase, state.suggestedText, state.error]);

  if (!open || !state.position) return null;

  const overlay = nodeRef.current?.closest(".selection-ai-overlay");
  const overlayW = overlay instanceof HTMLElement ? overlay.clientWidth : Infinity;
  const overlayH = overlay instanceof HTMLElement ? overlay.clientHeight : Infinity;

  const placement = state.position.previewPlacement;
  let left = state.position.previewLeft - size.width / 2;
  let top =
    placement === "above"
      ? state.position.previewTop - size.height
      : state.position.previewTop;

  if (Number.isFinite(overlayW)) {
    left = Math.min(overlayW - size.width - EDGE, Math.max(EDGE, left));
  }
  if (Number.isFinite(overlayH)) {
    top = Math.min(overlayH - size.height - EDGE, Math.max(EDGE, top));
  }

  const busy = state.phase === "loading";

  return (
    <Draggable
      key={dragKey}
      nodeRef={nodeRef}
      handle=".selection-ai-preview-head-drag"
      cancel="button, a, input, textarea, summary, pre, details"
      bounds=".selection-ai-overlay"
      defaultClassName="selection-ai-draggable"
      defaultClassNameDragging="selection-ai-dragging"
      disabled={busy}
    >
      <div
        ref={nodeRef}
        className="selection-ai-preview"
        style={{ top, left }}
        role="dialog"
        aria-label="Превью AI-правки"
        aria-busy={busy}
      >
        {state.phase === "error" ? (
          <>
            <div className="selection-ai-preview-head selection-ai-preview-head-drag">
              <GripVertical size={14} className="selection-ai-preview-grip" aria-hidden />
              <span className="selection-ai-preview-title">Ошибка</span>
            </div>
            <div className="selection-ai-preview-error">
              <p>{state.error ?? "Не удалось получить ответ модели"}</p>
              <div className="selection-ai-preview-actions">
                <button type="button" className="selection-ai-btn" onClick={onRetry}>
                  <RotateCcw size={14} />
                  Повторить
                </button>
                <button type="button" className="selection-ai-btn" onClick={onReject}>
                  <X size={14} />
                  Закрыть
                </button>
              </div>
            </div>
          </>
        ) : busy && !state.suggestedText ? (
          <>
            <div className="selection-ai-preview-head">
              <Loader2 size={14} className="selection-ai-spin" aria-hidden />
              <span className="selection-ai-preview-title">Генерация…</span>
            </div>
            <div className="selection-ai-preview-loading">
              Модель переписывает выделенный фрагмент
            </div>
            <div className="selection-ai-preview-actions">
              <button type="button" className="selection-ai-btn" onClick={onReject}>
                <X size={14} />
                Отмена
              </button>
            </div>
          </>
        ) : (
          <>
            <div className="selection-ai-preview-head selection-ai-preview-head-drag">
              {busy ? (
                <Loader2 size={14} className="selection-ai-spin" aria-hidden />
              ) : (
                <GripVertical size={14} className="selection-ai-preview-grip" aria-hidden />
              )}
              <span className="selection-ai-preview-title">
                {busy ? "Обновление…" : "Предложение"}
              </span>
              {!busy ? (
                <span className="selection-ai-preview-drag-hint">перетащите</span>
              ) : null}
            </div>
            <pre className="selection-ai-preview-suggestion">
              {state.suggestedText}
            </pre>
            <details className="selection-ai-preview-original">
              <summary>Исходный текст</summary>
              <pre className="selection-ai-preview-original-text">{state.selectedText}</pre>
            </details>
            <div className="selection-ai-preview-actions">
              <button
                type="button"
                className="selection-ai-btn"
                onClick={onReject}
                disabled={busy}
              >
                <X size={14} />
                Отклонить
              </button>
              <button
                type="button"
                className="selection-ai-btn"
                onClick={onRetry}
                disabled={busy}
              >
                <RotateCcw size={14} />
                Повторить
              </button>
              <button
                type="button"
                className="selection-ai-btn selection-ai-btn-primary"
                onClick={onAccept}
                disabled={busy}
              >
                <Check size={14} />
                Принять
              </button>
            </div>
          </>
        )}
      </div>
    </Draggable>
  );
}
