import * as MonacoNs from "monaco-editor";
import type * as Monaco from "monaco-editor";
import { useEffect, useRef } from "react";
import type { Diagnostic } from "../lib/workspaceIndex";

const WIDGET_ID = "docflow.errors-indicator";

/**
 * Индикатор количества ошибок/предупреждений в правом верхнем углу редактора
 * (как в IDEA). Реализован через штатный API Monaco `editor.addOverlayWidget`
 * — единственный нативный способ отрисовать произвольный HTML-вид поверх
 * редактора, который корректно переживает скролл и layout.
 *
 * Виджет считает ошибки/варнинги для активного документа (`activePath`) и
 * обновляется при каждом изменении `diagnostics`. Клик открывает Problems
 * panel (через колбэк `onOpenProblems`).
 */
export function useMonacoErrorsWidget(
  editor: Monaco.editor.IStandaloneCodeEditor | null,
  diagnostics: Diagnostic[],
  activePath: string | null,
  onOpenProblems: () => void,
) {
  const onClickRef = useRef(onOpenProblems);
  onClickRef.current = onOpenProblems;
  // Храним ссылку на DOM-ноду виджета, чтобы второй эффект мог обновлять
  // содержимое без обращения к недокументированному `getOverlayWidgets`.
  const domRef = useRef<HTMLDivElement | null>(null);

  // 1. Регистрация overlay-виджета в редакторе.
  useEffect(() => {
    if (!editor) return;

    const widget: Monaco.editor.IOverlayWidget = {
      getId: () => WIDGET_ID,
      getDomNode: () => {
        if (!domRef.current) {
          const el = document.createElement("div");
          el.className = "df-errors-widget";
          el.setAttribute("role", "button");
          el.setAttribute("tabindex", "0");
          el.title = "Открыть панель проблем";
          el.addEventListener("click", () => onClickRef.current());
          el.addEventListener("keydown", (e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              onClickRef.current();
            }
          });
          domRef.current = el;
        }
        return domRef.current;
      },
      getPosition: () => ({
        preference:
          MonacoNs.editor.OverlayWidgetPositionPreference.TOP_RIGHT_CORNER,
      }),
    };

    editor.addOverlayWidget(widget);
    return () => {
      editor.removeOverlayWidget(widget);
      domRef.current = null;
    };
  }, [editor]);

  // 2. Обновление содержимого виджета при изменении диагностик/активного пути.
  useEffect(() => {
    const el = domRef.current;
    if (!el || !activePath) return;

    const forThisDoc = diagnostics.filter((d) => d.document === activePath);
    const errors = forThisDoc.filter((d) => d.severity === "error").length;
    const warnings = forThisDoc.filter((d) => d.severity === "warning").length;

    el.classList.toggle("has-errors", errors > 0);
    el.classList.toggle("has-warnings", errors === 0 && warnings > 0);
    el.classList.toggle("is-clean", errors === 0 && warnings === 0);

    if (errors > 0) {
      el.innerHTML = renderIcon("error") + renderCount(errors, warnings);
    } else if (warnings > 0) {
      el.innerHTML = renderIcon("warning") + renderCount(0, warnings);
    } else {
      el.innerHTML = renderIcon("clean");
    }
  }, [diagnostics, activePath]);
}

function renderIcon(kind: "error" | "warning" | "clean"): string {
  const cls =
    kind === "error"
      ? "codicon codicon-error"
      : kind === "warning"
        ? "codicon codicon-warning"
        : "codicon codicon-pass";
  return `<span class="df-errors-widget-icon ${cls}"></span>`;
}

function renderCount(errors: number, warnings: number): string {
  const parts: string[] = [];
  if (errors > 0) parts.push(`<b>${errors}</b>`);
  if (warnings > 0) parts.push(`<span class="muted">${warnings}</span>`);
  return `<span class="df-errors-widget-text">${parts.join(" · ")}</span>`;
}
