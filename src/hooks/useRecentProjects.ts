import { useCallback, useEffect, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import { listRecentProjects, removeRecentProject, type RecentProject } from "../lib/project";

type Actions = {
  /** Opens a folder picker and the project behind it. Supplied by `App`. */
  onOpenFolder: () => Promise<unknown>;
  onOpenRecent: (root: string) => Promise<unknown>;
};

/** Just the recent-projects list and removing an entry — what both the
 * welcome screen and the TopBar dropdown need.
 *
 * A failing *list* is swallowed to an empty list rather than surfaced: both
 * surfaces still work without history, and an error banner in front of them
 * would be noise. */
export function useRecentProjectsList() {
  const [recent, setRecent] = useState<RecentProject[]>([]);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const reload = useCallback(async () => {
    try {
      const items = await listRecentProjects();
      if (mounted.current) setRecent(items);
    } catch {
      if (mounted.current) setRecent([]);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const removeRecent = useCallback(
    async (root: string) => {
      try {
        await removeRecentProject(root);
        await reload();
      } catch (e) {
        if (mounted.current) setError(toMessage(e));
      }
    },
    [reload],
  );

  return { recent, reload, removeRecent, listError: error, mounted };
}

/** `useRecentProjectsList` plus opening a project, for the welcome screen.
 *
 * A failing *open* is surfaced, unlike a failing list — the user asked for
 * that one explicitly. */
export function useRecentProjects({ onOpenFolder, onOpenRecent }: Actions) {
  const { recent, reload, removeRecent, mounted } = useRecentProjectsList();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const openFolder = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await onOpenFolder();
    } catch (e) {
      if (mounted.current) setError(toMessage(e));
    } finally {
      if (mounted.current) setBusy(false);
    }
  }, [onOpenFolder, mounted]);

  const openRecent = useCallback(
    async (root: string) => {
      setBusy(true);
      setError(null);
      try {
        await onOpenRecent(root);
      } catch (e) {
        if (!mounted.current) return;
        setError(toMessage(e));
        // The entry may be gone from disk — refresh so the list stops
        // offering something that cannot be opened.
        await reload();
      } finally {
        if (mounted.current) setBusy(false);
      }
    },
    [onOpenRecent, reload, mounted],
  );

  return { recent, busy, error, openFolder, openRecent, removeRecent };
}
