import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { playCalendarReminder } from "../lib/assistantSounds";
import { formatEventTime, getCalendarSettings, type CalendarReminder } from "../lib/calendar";

/** Chimes ahead of a meeting. The countdown, the lead time and the once-only
 * guard all live in Rust (`CalendarState::claim_due_reminders`); this only
 * turns the notice into a sound and a banner.
 *
 * Mounted in `App`, deliberately not in the calendar panel — that panel
 * unmounts whenever the dock is hidden or shows another tool. */
export function useCalendarReminders(): void {
  useEffect(() => {
    const unlisten = listen<CalendarReminder>("calendar:reminder", async ({ payload }) => {
      // Read per reminder rather than held in state: this fires a handful of
      // times a day, and a display zone changed in settings applies at once.
      let tz = "";
      try {
        tz = (await getCalendarSettings()).settings.displayTimeZone;
      } catch {
        /* fall back to the OS zone */
      }
      playCalendarReminder(
        payload.event.title,
        `Начало в ${formatEventTime(payload.event.start, tz)}, через ${payload.minutes} мин`,
      );
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);
}
