import { useCallback, useEffect, useState } from "react";
import { toMessage } from "../lib/errors";
import { getAiAccessMode, setAiAccessMode, type AiAccessMode } from "../lib/aiTools";

/** Drives the docs-only/full-repo toggle for the currently open project.
 * Small and standalone rather than folded into `useEmbeddingSetup` — the
 * access mode is a general AI-harness boundary (`ai_execute_tool` reads it
 * too), embeddings are just its first consumer with UI. */
export function useAiAccessMode() {
  const [mode, setModeState] = useState<AiAccessMode | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setModeState(await getAiAccessMode());
      setError(null);
    } catch (e) {
      setError(toMessage(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const setMode = useCallback(async (next: AiAccessMode) => {
    const previous = mode;
    setModeState(next);
    setBusy(true);
    try {
      await setAiAccessMode(next);
      setError(null);
    } catch (e) {
      setError(toMessage(e));
      setModeState(previous);
    } finally {
      setBusy(false);
    }
  }, [mode]);

  return { mode, busy, error, setMode, refresh };
}
