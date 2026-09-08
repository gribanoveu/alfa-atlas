import { useEffect, useState } from "react";
import {
  getToolDefinitions,
  type AiAccessMode,
  type ConversationMode,
  type LlmToolDefinition,
} from "../lib/aiTools";

/** Fetches the tool definitions currently allowed for the open project
 * (same source the backend uses for real function-calling), so the system
 * prompt's "Tool usage" section can be generated from live data instead of
 * hardcoded prose. Refetches on `accessMode`/`conversationMode` change,
 * mirroring `useSpecsRepo`'s shape.
 *
 * `repoRoot` is a refetch trigger, not an argument to the call (the backend
 * resolves the open project itself): with no project the backend answers
 * with the project-free tools only, and `assistantConfig`'s
 * `resolveAccessLevel` reads that list as "no project open". `AssistantPanel`
 * mounts before a project is opened and never remounts, so without this the
 * assistant would keep claiming there is no project — and `accessMode`
 * alone can't stand in for it, since it lands on the same `docsOnly` the
 * no-project fallback already used. */
export function useToolDefinitions(
  accessMode: AiAccessMode,
  conversationMode: ConversationMode,
  repoRoot: string | null,
) {
  const [definitions, setDefinitions] = useState<LlmToolDefinition[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getToolDefinitions(conversationMode)
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
  }, [accessMode, conversationMode, repoRoot]);

  return { definitions, loading };
}
