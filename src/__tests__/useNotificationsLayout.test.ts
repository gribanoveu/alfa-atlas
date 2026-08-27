import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { GeneralPrefs } from "../lib/prefs";
import * as actualPrefs from "../lib/prefs";

let stored: GeneralPrefs;
let writes: GeneralPrefs[] = [];

mock.module("../lib/prefs", () => ({
  ...actualPrefs,
  getGeneralPrefs: async () => stored,
  setGeneralPrefs: async (next: GeneralPrefs) => {
    writes.push(next);
    stored = next;
  },
}));

const { useNotificationsLayout } = await import("../hooks/useNotificationsLayout");

beforeEach(() => {
  stored = { ...actualPrefs.DEFAULT_GENERAL_PREFS };
  writes = [];
});

describe("useNotificationsLayout", () => {
  test("restores the last expanded state", async () => {
    stored = {
      ...actualPrefs.DEFAULT_GENERAL_PREFS,
      notificationsAlertsExpanded: false,
      notificationsOnboardingExpanded: false,
    };
    const { result } = renderHook(() => useNotificationsLayout());

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.alertsExpanded).toBe(false);
    expect(result.current.onboardingExpanded).toBe(false);
  });

  test("toggling a section persists it", async () => {
    const { result } = renderHook(() => useNotificationsLayout());
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.toggleAlerts());
    expect(result.current.alertsExpanded).toBe(false);

    await waitFor(() => expect(writes.length).toBeGreaterThan(0));
    expect(writes.at(-1)).toMatchObject({
      notificationsAlertsExpanded: false,
      notificationsOnboardingExpanded: true,
    });

    act(() => result.current.toggleOnboarding());
    expect(result.current.onboardingExpanded).toBe(false);

    await waitFor(() =>
      expect(writes.at(-1)?.notificationsOnboardingExpanded).toBe(false),
    );
  });
});
