import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { GeneralPrefs } from "../lib/prefs";
import * as actualPrefs from "../lib/prefs";

/** Full prefs snapshot — do not use `actualPrefs.DEFAULT_GENERAL_PREFS` here; other
 *  test files mock that export with partial objects and the mock leaks across files. */
const BASE_PREFS: GeneralPrefs = {
  restoreLastProject: true,
  autosaveEnabled: true,
  saveOnTabSwitch: true,
  autosaveDelayMs: 1000,
  separateExternalFolder: true,
  openApiRefFallbackEnabled: true,
  errorLanguage: "ru",
  uiFontSizePx: 12.5,
  sidebarFontSizePx: 12,
  editorFontSizePx: 13,
  previewFontSizePx: 14,
  assistantFontSizePx: 13,
  lastCloneDir: null,
  notificationsAlertsExpanded: true,
  notificationsOnboardingExpanded: true,
};

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
  stored = { ...BASE_PREFS };
  writes = [];
});

describe("useNotificationsLayout", () => {
  test("restores the last expanded state", async () => {
    stored = {
      ...BASE_PREFS,
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
