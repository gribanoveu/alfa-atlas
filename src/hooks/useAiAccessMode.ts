import { useCallback, useEffect, useState } from "react";
import { toMessage } from "../lib/errors";
import { getAiAccessMode, setAiAccessMode, type AiAccessMode } from "../lib/aiTools";

/** Drives the docs-only/full-repo toggle for the currently open project.
 * Small and standalone rather than folded into `useEmbeddingSetup` — the
 * access mode is a general AI-harness boundary (`ai_execute_tool` reads it
 * too), embeddings are just its first consumer with UI.
 *
 * `repoRoot` is the project this mode belongs to, and re-reading on it is
 * the point: `AssistantPanel` mounts once and stays mounted (the chat works
 * with no project open), so a project opened afterwards would otherwise
 * leave `mode` stuck at its no-project `null` — which reads as "still
 * loading" and disables the toggle for the rest of the session. */
export function useAiAccessMode(repoRoot: string | null) {
  const [mode, setModeState] = useState<AiAccessMode | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    // No project: the backend has no mode to report ("no project is open"),
    // and there is nothing to toggle — not an error worth showing.
    if (!repoRoot) {
      setModeState(null);
      setError(null);
      return;
    }
    try {
      setModeState(await getAiAccessMode());
      setError(null);
    } catch (e) {
      setError(toMessage(e));
    }
  }, [repoRoot]);

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
