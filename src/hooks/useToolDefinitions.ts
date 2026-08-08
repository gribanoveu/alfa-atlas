import { useEffect, useState } from "react";
import { getToolDefinitions, type AiAccessMode, type LlmToolDefinition } from "../lib/aiTools";

/** Fetches the tool definitions currently allowed for the open project
 * (same source the backend uses for real function-calling), so the system
 * prompt's "Tool usage" section can be generated from live data instead of
 * hardcoded prose. Refetches on `accessMode` change, mirroring
 * `useSpecsRepo`'s shape. */
export function useToolDefinitions(accessMode: AiAccessMode) {
  const [definitions, setDefinitions] = useState<LlmToolDefinition[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getToolDefinitions()
      .then((result) => {
        if (!cancelled) setDefinitions(result);
      })
      .catch(() => {
        if (!cancelled) setDefinitions([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [accessMode]);

  return { definitions, loading };
}
