import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toMessage } from "../lib/errors";
import {
  getCachedCalendarEvents,
  getCalendarStatus,
  syncCalendar,
  type CalendarEvent,
  type CalendarStatus,
} from "../lib/calendar";

/** Live calendar state for the right-dock panel. The cache and sync loop live
 * in Rust (the panel unmounts when hidden), so this hook just mirrors them:
 * shows the cache instantly on mount, kicks a refresh, and stays current via
 * the backend's `calendar:updated` event even while other work happens. */
export function useCalendarEvents() {
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [status, setStatus] = useState<CalendarStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const runId = useRef(0);

  const reloadStatus = useCallback(async () => {
    try {
      setStatus(await getCalendarStatus());
    } catch {
      /* status is best-effort */
    }
  }, []);

  const refresh = useCallback(async () => {
    const id = ++runId.current;
    setLoading(true);
    setError(null);
    try {
      const fresh = await syncCalendar();
      if (id !== runId.current) return;
      setEvents(fresh);
    } catch (e) {
      if (id !== runId.current) return;
      setError(toMessage(e));
    } finally {
      if (id === runId.current) setLoading(false);
      void reloadStatus();
    }
  }, [reloadStatus]);

  useEffect(() => {
    void (async () => {
      try {
        setEvents(await getCachedCalendarEvents());
      } catch {
        /* empty cache is fine */
      }
      await reloadStatus();
      void refresh();
    })();

    const unlisten = listen<CalendarEvent[]>("calendar:updated", (e) => {
      setEvents(e.payload);
      void reloadStatus();
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [refresh, reloadStatus]);

  return { events, status, loading, error, refresh, reloadStatus };
}
