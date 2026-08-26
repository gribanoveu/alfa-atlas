import { useCallback, useEffect, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import { listRecentProjects, removeRecentProject, type RecentProject } from "../lib/project";

type Actions = {
  /** Opens a folder picker and the project behind it. Supplied by `App`. */
  onOpenFolder: () => Promise<unknown>;
  onOpenRecent: (root: string) => Promise<unknown>;
};

/** The recent-projects list on the welcome screen, and opening one.
 *
 * A failing *list* is swallowed to an empty list rather than surfaced: the
 * welcome screen still works without history, and an error banner there
 * would be noise in front of the two buttons that matter. A failing *open*
 * is surfaced, since the user asked for it explicitly. */
export function useRecentProjects({ onOpenFolder, onOpenRecent }: Actions) {
  const [recent, setRecent] = useState<RecentProject[]>([]);
  const [busy, setBusy] = useState(false);
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
  }, [onOpenFolder]);

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
    [onOpenRecent, reload],
  );

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

  return { recent, busy, error, openFolder, openRecent, removeRecent };
}
