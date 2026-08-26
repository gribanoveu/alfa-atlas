import { useCallback, useEffect, useRef, useState } from "react";
import {
  getAllowedTools,
  getAutoApprovedTools,
  setToolAllowed,
  setToolAutoApproved,
} from "../lib/aiTools";
import { toMessage } from "../lib/errors";

/** One project-scoped list of tool names, plus its loading/no-project/error
 * state and a pending-toggle marker. */
export type ToolList = {
  tools: string[];
  loading: boolean;
  /** No project is open — a normal state on a healthy install, not an error,
   * so the tab renders a plain hint instead of an error banner. */
  noProject: boolean;
  error: string | null;
  /** The tool whose write is in flight, so the caller can disable just that
   * row rather than the whole list. */
  pending: string | null;
};

const EMPTY: ToolList = {
  tools: [],
  loading: true,
  noProject: false,
  error: null,
  pending: null,
};

/** "No project is open" comes back as an ordinary command error string; it
 * is the one failure this tab treats as a state rather than a fault. */
function isNoProject(message: string): boolean {
  return message.includes("no project is open");
}

/** The two project-scoped tool lists the permissions tab shows: which tools
 * the assistant may use at all, and which it may use without asking.
 *
 * Both are the same shape — load, degrade on "no project", toggle one row at
 * a time — so they share one implementation here instead of the two
 * near-identical `useEffect` blocks the component used to hold. */
export function useToolPermissions() {
  const [autoApproved, setAutoApproved] = useState<ToolList>(EMPTY);
  const [allowed, setAllowed] = useState<ToolList>(EMPTY);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    const load = async (
      fetch: () => Promise<string[]>,
      apply: (next: React.SetStateAction<ToolList>) => void,
    ) => {
      try {
        const tools = await fetch();
        if (!mounted.current) return;
        apply((prev) => ({ ...prev, tools, noProject: false, error: null }));
      } catch (e) {
        if (!mounted.current) return;
        const message = toMessage(e);
        apply((prev) =>
          isNoProject(message)
            ? { ...prev, noProject: true }
            : { ...prev, error: message },
        );
      } finally {
        if (mounted.current) apply((prev) => ({ ...prev, loading: false }));
      }
    };
    void load(getAutoApprovedTools, setAutoApproved);
    void load(getAllowedTools, setAllowed);
  }, []);

  /** Drops a tool's standing approval, so the assistant has to ask again. */
  const revokeAutoApproval = useCallback(async (tool: string) => {
    setAutoApproved((prev) => ({ ...prev, pending: tool }));
    try {
      await setToolAutoApproved(tool, false);
      if (!mounted.current) return;
      setAutoApproved((prev) => ({
        ...prev,
        tools: prev.tools.filter((t) => t !== tool),
      }));
    } catch (e) {
      if (mounted.current) {
        setAutoApproved((prev) => ({ ...prev, error: toMessage(e) }));
      }
    } finally {
      if (mounted.current) setAutoApproved((prev) => ({ ...prev, pending: null }));
    }
  }, []);

  const toggleAllowed = useCallback(async (tool: string, next: boolean) => {
    setAllowed((prev) => ({ ...prev, pending: tool }));
    try {
      await setToolAllowed(tool, next);
      if (!mounted.current) return;
      setAllowed((prev) => ({
        ...prev,
        tools: next ? [...prev.tools, tool] : prev.tools.filter((t) => t !== tool),
      }));
    } catch (e) {
      if (mounted.current) setAllowed((prev) => ({ ...prev, error: toMessage(e) }));
    } finally {
      if (mounted.current) setAllowed((prev) => ({ ...prev, pending: null }));
    }
  }, []);

  return { autoApproved, allowed, revokeAutoApproval, toggleAllowed };
}
