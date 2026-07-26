import { useCallback, useState } from "react";
import type { EditorViewMode } from "../types/viewMode";

/**
 * Глобальный режим просмотра редактора: исходник / сплит / рендер.
 *
 * Состояние хранится в памяти сессии (без персистентности) и применяется
 * ко всем вкладкам одновременно — как режим preview в IDE.
 */
export function useEditorViewMode() {
  const [viewMode, setViewMode] = useState<EditorViewMode>("source");

  const changeViewMode = useCallback((mode: EditorViewMode) => {
    setViewMode((prev) => (prev === mode ? prev : mode));
  }, []);

  return { viewMode, setViewMode: changeViewMode };
}
