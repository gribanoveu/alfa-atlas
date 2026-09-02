import { useCallback, useEffect, useState } from "react";
import { toMessage } from "../lib/errors";
import {
  getJiraSettings,
  setJiraSettings,
  type JiraProject,
  type JiraSettings,
} from "../lib/jira";

/** Just the remembered project, for surfaces that have no business editing
 *  the rest of the Jira settings — the right-dock panel.
 *
 *  Reads and writes the same `settings.json` the settings tab does, so the
 *  two cannot disagree about what is stored. They can briefly disagree about
 *  what is *shown*: a change made in one does not push into the other, and
 *  each re-reads when it next mounts. That is acceptable here because the
 *  panel remounts every time it is opened (see `useJiraConnection`), and the
 *  alternative — a shared store for one string — is not worth it. */
export function useJiraProject() {
  const [settings, setSettings] = useState<JiraSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getJiraSettings()
      .then((view) => {
        if (!cancelled) setSettings(view.settings);
      })
      .catch((e) => {
        if (!cancelled) setError(toMessage(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const pick = useCallback(
    async (project: JiraProject) => {
      if (!settings) return;
      const next: JiraSettings = {
        ...settings,
        projectKey: project.key,
        projectName: project.name,
      };
      // Optimistic: the row updates immediately and a failed write rolls
      // back to whatever the backend really holds, rather than leaving the
      // panel claiming a project that was never saved.
      setSettings(next);
      setBusy(true);
      try {
        await setJiraSettings(next);
        setError(null);
      } catch (e) {
        setError(toMessage(e));
        const current = await getJiraSettings().catch(() => null);
        if (current) setSettings(current.settings);
      } finally {
        setBusy(false);
      }
    },
    [settings],
  );

  return {
    projectKey: settings?.projectKey ?? "",
    projectName: settings?.projectName ?? "",
    ready: settings !== null,
    busy,
    error,
    pick,
  };
}
