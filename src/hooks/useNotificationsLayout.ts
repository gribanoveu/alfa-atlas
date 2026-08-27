import { useCallback, useEffect, useRef, useState } from "react";
import { getGeneralPrefs, setGeneralPrefs } from "../lib/prefs";

/** Expanded/collapsed state of the Notifications tab sections.
 *
 * Persisted with general prefs (same store as `lastCloneDir`) so it survives
 * leaving the tab and app restarts, including when no project is open. */
export function useNotificationsLayout() {
  const [alertsExpanded, setAlertsExpanded] = useState(true);
  const [onboardingExpanded, setOnboardingExpanded] = useState(true);
  const [ready, setReady] = useState(false);
  const userTouched = useRef(false);

  useEffect(() => {
    let cancelled = false;
    getGeneralPrefs()
      .then((prefs) => {
        if (cancelled || userTouched.current) return;
        setAlertsExpanded(prefs.notificationsAlertsExpanded ?? true);
        setOnboardingExpanded(prefs.notificationsOnboardingExpanded ?? true);
      })
      .catch(() => {
        // First-run / missing prefs: keep both sections expanded.
      })
      .finally(() => {
        if (!cancelled) setReady(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const persist = useCallback(
    (patch: {
      notificationsAlertsExpanded?: boolean;
      notificationsOnboardingExpanded?: boolean;
    }) => {
      void getGeneralPrefs()
        .then((prefs) => setGeneralPrefs({ ...prefs, ...patch }))
        .catch(() => {
          // Ignore persistence failures; the in-memory toggle still stands.
        });
    },
    [],
  );

  const toggleAlerts = useCallback(() => {
    userTouched.current = true;
    setAlertsExpanded((current) => {
      const next = !current;
      persist({ notificationsAlertsExpanded: next });
      return next;
    });
  }, [persist]);

  const toggleOnboarding = useCallback(() => {
    userTouched.current = true;
    setOnboardingExpanded((current) => {
      const next = !current;
      persist({ notificationsOnboardingExpanded: next });
      return next;
    });
  }, [persist]);

  return {
    ready,
    alertsExpanded,
    onboardingExpanded,
    toggleAlerts,
    toggleOnboarding,
  };
}
