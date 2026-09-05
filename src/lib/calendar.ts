import { invoke } from "@tauri-apps/api/core";

/** Mirrors `domain::calendar::MeetingPlatform`. */
export type MeetingPlatform =
  | "teams"
  | "zoom"
  | "googleMeet"
  | "webex"
  | "telemost"
  | "jazz"
  | "vk"
  | "ktalk"
  | "mtsLink"
  | "generic";

/** Mirrors `domain::calendar::MeetingResponseType`. */
export type MeetingResponseType =
  | "accepted"
  | "tentative"
  | "declined"
  | "organizer"
  | "notResponded";

/** Mirrors `domain::calendar::CalendarEvent`. `start`/`end` are RFC3339 UTC.
 *  `joinUrl` is already validated to https by the backend — safe to open. */
export type CalendarEvent = {
  id: string;
  /** Revision key, needed to fetch details. May be null. */
  changeKey: string | null;
  title: string;
  start: string;
  end: string;
  isAllDay: boolean;
  isCancelled: boolean;
  isOrganizer: boolean;
  organizer: string | null;
  location: string | null;
  joinUrl: string | null;
  platform: MeetingPlatform;
  responseType: MeetingResponseType;
};

/** Mirrors `domain::calendar::EventAttendee`. */
export type EventAttendee = {
  name: string;
  email: string | null;
  required: boolean;
  response: MeetingResponseType;
};

/** Mirrors `domain::calendar::EventDetails` — fetched when a meeting expands. */
export type EventDetails = {
  attendees: EventAttendee[];
  bodyText: string | null;
};

/** Payload of the `calendar:reminder` event (`commands::calendar_events`).
 *  `minutes` is how far out the meeting is, already worded for the banner. */
export type CalendarReminder = {
  event: CalendarEvent;
  minutes: number;
};

/** Mirrors `domain::calendar::CalendarStatus`. */
export type CalendarStatus = {
  configured: boolean;
  connected: boolean;
  circuitOpen: boolean;
  cachedCount: number;
  lastError: string | null;
};

/** Mirrors `domain::calendar::CalendarSettings`. Password is never here. */
export type CalendarSettings = {
  baseUrl: string;
  username: string;
  displayTimeZone: string;
  rememberPassword: boolean;
  trustedCertPem: string | null;
  /** Chime this many minutes before a meeting starts; 0 is off. */
  reminderMinutes: number;
};

/** Mirrors `domain::calendar::CalendarSettingsView`. */
export type CalendarSettingsView = {
  settings: CalendarSettings;
  bundledBaseUrl: string | null;
  hasBundledCert: boolean;
  hasPassword: boolean;
};

export function getCalendarSettings(): Promise<CalendarSettingsView> {
  return invoke<CalendarSettingsView>("calendar_get_settings");
}

export function saveCalendarSettings(settings: CalendarSettings): Promise<void> {
  return invoke<void>("calendar_save_settings", { settings });
}

/** Write-only: the password never comes back over IPC. */
export function setCalendarPassword(password: string, remember: boolean): Promise<void> {
  return invoke<void>("calendar_set_password", { password, remember });
}

export function calendarHasPassword(): Promise<boolean> {
  return invoke<boolean>("calendar_has_password");
}

export function forgetCalendarPassword(): Promise<void> {
  return invoke<void>("calendar_forget");
}

export function getCachedCalendarEvents(): Promise<CalendarEvent[]> {
  return invoke<CalendarEvent[]>("calendar_get_cached");
}

export function getCalendarStatus(): Promise<CalendarStatus> {
  return invoke<CalendarStatus>("calendar_status");
}

/** Fetches now + refreshes the cache. Also the connection check. */
export function syncCalendar(): Promise<CalendarEvent[]> {
  return invoke<CalendarEvent[]>("calendar_sync");
}

/** Mirrors `domain::calendar::RsvpAction`. */
export type RsvpAction = "accept" | "tentative" | "decline";

/** Sends an RSVP. `send` notifies the organizer (SendAndSaveCopy); `false`
 * updates only the user's own calendar (SaveOnly — silent). */
export function calendarRsvp(
  id: string,
  changeKey: string,
  action: RsvpAction,
  send: boolean,
): Promise<void> {
  return invoke<void>("calendar_rsvp", { id, changeKey, action, send });
}

/** Attendees + body for one meeting, fetched when it is expanded. */
export function calendarEventDetails(
  id: string,
  changeKey: string | null,
): Promise<EventDetails> {
  return invoke<EventDetails>("calendar_event_details", { id, changeKey });
}

/** Whether a request is worth attempting: a host is set (own or bundled). */
export function isCalendarAddressable(view: CalendarSettingsView): boolean {
  return Boolean(view.settings.baseUrl.trim() || view.bundledBaseUrl);
}

/** Cancelled by the server flag, or by a cancellation subject prefix (common
 *  when the cancellation arrives via an external mail client). A cancelled
 *  meeting shows struck-through and offers no join link. */
export function isEffectivelyCancelled(e: CalendarEvent): boolean {
  if (e.isCancelled) return true;
  const t = e.title.trim().toLowerCase();
  return t.startsWith("отменено:") || t.startsWith("cancelled:") || t.startsWith("canceled:");
}

/** `undefined` tz makes `Intl` use the viewer's OS zone — which is exactly
 *  what an empty `displayTimeZone` means. */
function zone(tz: string): string | undefined {
  return tz.trim() || undefined;
}

/** Stable per-day key ("2026-09-04") in the display zone. */
function dayKey(iso: string, tz: string): string {
  return new Intl.DateTimeFormat("en-CA", {
    timeZone: zone(tz),
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(new Date(iso));
}

/** Day key ("2026-09-04") of an instant in the display zone. */
export function tzDayKey(iso: string, tz: string): string {
  return dayKey(iso, tz);
}

/** Fractional hour-of-day (0–24) of an instant in the display zone, e.g. 14.5
 * for 14:30. Used to position events on the day timeline. */
export function tzHourFraction(iso: string, tz: string): number {
  const parts = new Intl.DateTimeFormat("en-GB", {
    timeZone: zone(tz),
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).formatToParts(new Date(iso));
  const h = Number(parts.find((p) => p.type === "hour")?.value ?? "0") % 24;
  const m = Number(parts.find((p) => p.type === "minute")?.value ?? "0");
  return h + m / 60;
}

export function formatEventTime(iso: string, tz: string): string {
  return new Intl.DateTimeFormat("ru-RU", {
    timeZone: zone(tz),
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(iso));
}

export function formatDayLabel(iso: string, tz: string): string {
  return new Intl.DateTimeFormat("ru-RU", {
    timeZone: zone(tz),
    weekday: "long",
    day: "numeric",
    month: "long",
  }).format(new Date(iso));
}

/** Weekday name only, e.g. "суббота". */
export function formatWeekday(iso: string, tz: string): string {
  return new Intl.DateTimeFormat("ru-RU", { timeZone: zone(tz), weekday: "long" }).format(
    new Date(iso),
  );
}

/** Day + month only, e.g. "5 сентября". */
export function formatDayMonth(iso: string, tz: string): string {
  return new Intl.DateTimeFormat("ru-RU", {
    timeZone: zone(tz),
    day: "numeric",
    month: "long",
  }).format(new Date(iso));
}

export type CalendarDay = { key: string; label: string; events: CalendarEvent[] };

/** Groups events into day buckets in the display zone, from today onward,
 *  each sorted by start. The cache spans a wider UTC window than we show, so
 *  past days are dropped here. */
export function groupEventsByDay(events: CalendarEvent[], tz: string): CalendarDay[] {
  const today = dayKey(new Date().toISOString(), tz);
  const byDay = new Map<string, CalendarEvent[]>();
  for (const e of events) {
    const key = dayKey(e.start, tz);
    if (key < today) continue;
    (byDay.get(key) ?? byDay.set(key, []).get(key)!).push(e);
  }
  return [...byDay.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, evs]) => ({
      key,
      label: formatDayLabel(evs[0].start, tz),
      events: evs.sort((a, b) => a.start.localeCompare(b.start)),
    }));
}
