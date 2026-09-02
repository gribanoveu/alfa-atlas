import { useCallback, useEffect, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import {
  getJiraSettings,
  isJiraAddressable,
  jiraCurrentUser,
  jiraHasToken,
  type JiraUser,
} from "../lib/jira";

export type JiraConnection =
  | { kind: "idle" }
  | { kind: "loading" }
  /** Nothing to check yet — the panel points at Settings instead of showing
   * an error the user can't act on. `missing` says which half is absent. */
  | { kind: "unconfigured"; missing: "instance" | "token" }
  | { kind: "connected"; user: JiraUser }
  | { kind: "error"; message: string };

/** Live state of the Jira connection for the right-dock panel: who the
 * stored token belongs to, or why nothing can be shown.
 *
 * Checks once on mount, and the panel is mounted only while it is the
 * visible dock tool — so a dock the user never opened issues no request at
 * all, and reopening re-checks rather than showing a cached result. That
 * matters because the settings dialog can change the instance, the token or
 * the certificate while the panel is closed, and the panel has no way to
 * observe that. */
export function useJiraConnection() {
  const [state, setState] = useState<JiraConnection>({ kind: "idle" });
  // Guards against a slow check landing after a newer one — the panel would
  // otherwise flip back to a stale result.
  const runId = useRef(0);

  const check = useCallback(async () => {
    const id = ++runId.current;
    setState({ kind: "loading" });
    try {
      const [view, hasToken] = await Promise.all([getJiraSettings(), jiraHasToken()]);
      if (id !== runId.current) return;

      if (!isJiraAddressable(view)) {
        setState({ kind: "unconfigured", missing: "instance" });
        return;
      }
      if (!hasToken) {
        setState({ kind: "unconfigured", missing: "token" });
        return;
      }

      const user = await jiraCurrentUser();
      if (id !== runId.current) return;
      setState({ kind: "connected", user });
    } catch (e) {
      if (id !== runId.current) return;
      setState({ kind: "error", message: toMessage(e) });
    }
  }, []);

  useEffect(() => {
    void check();
  }, [check]);

  return { state, refresh: check };
}
