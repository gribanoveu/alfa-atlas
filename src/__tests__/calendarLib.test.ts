import { test, expect } from "bun:test";
import {
  groupEventsByDay,
  isEffectivelyCancelled,
  type CalendarEvent,
} from "../lib/calendar";

const ev = (over: Partial<CalendarEvent>): CalendarEvent => ({
  id: "1", changeKey: null, title: "M", start: "2999-01-02T09:00:00Z", end: "2999-01-02T10:00:00Z",
  isAllDay: false, isCancelled: false, isOrganizer: false, organizer: null,
  location: null, joinUrl: null, platform: "generic", responseType: "accepted",
  ...over,
});

test("groups by day and sorts within a day (UTC zone)", () => {
  const events = [
    ev({ id: "b", start: "2999-01-02T11:00:00Z" }),
    ev({ id: "a", start: "2999-01-02T08:00:00Z" }),
    ev({ id: "c", start: "2999-01-03T09:00:00Z" }),
  ];
  const days = groupEventsByDay(events, "UTC");
  expect(days.map((d) => d.key)).toEqual(["2999-01-02", "2999-01-03"]);
  expect(days[0].events.map((e) => e.id)).toEqual(["a", "b"]); // sorted by start
});

test("timezone shifts the day boundary", () => {
  // 22:00Z on Jan 2 is Jan 3 in Moscow (+3).
  const [day] = groupEventsByDay([ev({ start: "2999-01-02T22:00:00Z" })], "Europe/Moscow");
  expect(day.key).toBe("2999-01-03");
});

test("past days are dropped", () => {
  expect(groupEventsByDay([ev({ start: "2000-01-01T09:00:00Z" })], "UTC")).toHaveLength(0);
});

test("isEffectivelyCancelled: flag or subject prefix", () => {
  expect(isEffectivelyCancelled(ev({ isCancelled: true }))).toBe(true);
  expect(isEffectivelyCancelled(ev({ title: "Отменено: Standup" }))).toBe(true);
  expect(isEffectivelyCancelled(ev({ title: "cancelled: sync" }))).toBe(true);
  expect(isEffectivelyCancelled(ev({ title: "Standup" }))).toBe(false);
});

import { tzHourFraction, tzDayKey } from "../lib/calendar";

test("tzHourFraction / tzDayKey position events in the display zone", () => {
  // 07:30Z in UTC → 7.5h, same day.
  expect(tzHourFraction("2026-09-04T07:30:00Z", "UTC")).toBeCloseTo(7.5, 5);
  expect(tzDayKey("2026-09-04T07:30:00Z", "UTC")).toBe("2026-09-04");
  // 07:30Z in Moscow (+3) → 10.5h.
  expect(tzHourFraction("2026-09-04T07:30:00Z", "Europe/Moscow")).toBeCloseTo(10.5, 5);
  // 22:00Z is next day 01:00 in Moscow.
  expect(tzHourFraction("2026-09-04T22:00:00Z", "Europe/Moscow")).toBeCloseTo(1, 5);
  expect(tzDayKey("2026-09-04T22:00:00Z", "Europe/Moscow")).toBe("2026-09-05");
});
