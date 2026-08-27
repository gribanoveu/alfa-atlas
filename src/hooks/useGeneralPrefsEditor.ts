import { openPath } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import {
  DEFAULT_GENERAL_PREFS,
  getGeneralPrefs,
  getSettingsPaths,
  setGeneralPrefs,
  type GeneralPrefs,
  type SettingsPaths,
} from "../lib/prefs";

/** The settings dialog's editable view of general preferences.
 *
 * Distinct from `useGeneralPrefs`, which is the app-wide *read* of the same
 * data and falls back to defaults so the UI always has something to render.
 * This one owns writes, so it keeps `prefs` `null` until the real values
 * load — the dialog must not show a default the user never chose and then
 * silently persist it.
 *
 * Writes are optimistic with rollback: the control moves at once, and a
 * failed write restores whatever the backend actually holds. */
export function useGeneralPrefsEditor(
  onPrefsChange?: (prefs: GeneralPrefs) => void,
) {
  const [prefs, setPrefs] = useState<GeneralPrefs | null>(null);
  const [paths, setPaths] = useState<SettingsPaths | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const [nextPrefs, nextPaths] = await Promise.all([
          getGeneralPrefs(),
          getSettingsPaths(),
        ]);
        if (!mounted.current) return;
        setPrefs(nextPrefs);
        setPaths(nextPaths);
        setError(null);
      } catch (e) {
        if (mounted.current) setError(toMessage(e));
      }
    })();
  }, []);

  const persistPrefs = useCallback(
    async (next: GeneralPrefs) => {
      setPrefs(next);
      setBusy(true);
      try {
        await setGeneralPrefs(next);
        onPrefsChange?.(next);
        if (mounted.current) setError(null);
      } catch (e) {
        if (!mounted.current) return;
        setError(toMessage(e));
        const current = await getGeneralPrefs().catch(() => null);
        if (current && mounted.current) setPrefs(current);
      } finally {
        if (mounted.current) setBusy(false);
      }
    },
    [onPrefsChange],
  );

  const patchPrefs = useCallback(
    (patch: Partial<GeneralPrefs>) => {
      if (!prefs) return;
      void persistPrefs({ ...prefs, ...patch });
    },
    [persistPrefs, prefs],
  );

  /** Applies a change live without writing it — a slider previews as it is
   * dragged, and `persistPref` saves once the drag ends. */
  const stagePref = useCallback(
    (patch: Partial<GeneralPrefs>) => {
      if (!prefs) return;
      const next = { ...prefs, ...patch };
      setPrefs(next);
      onPrefsChange?.(next);
    },
    [onPrefsChange, prefs],
  );

  const resetFontPrefs = useCallback(() => {
    if (!prefs) return;
    void persistPrefs({
      ...prefs,
      uiFontSizePx: DEFAULT_GENERAL_PREFS.uiFontSizePx,
      sidebarFontSizePx: DEFAULT_GENERAL_PREFS.sidebarFontSizePx,
      editorFontSizePx: DEFAULT_GENERAL_PREFS.editorFontSizePx,
      previewFontSizePx: DEFAULT_GENERAL_PREFS.previewFontSizePx,
      assistantFontSizePx: DEFAULT_GENERAL_PREFS.assistantFontSizePx,
    });
  }, [persistPrefs, prefs]);

  /** Reveals the settings folder in the OS file manager. */
  const openUserSettingsDir = useCallback(async () => {
    if (!paths?.userSettingsDir) return;
    try {
      await openPath(paths.userSettingsDir);
    } catch (e) {
      if (mounted.current) setError(toMessage(e));
    }
  }, [paths]);

  return {
    prefs,
    paths,
    error,
    busy,
    patchPrefs,
    stagePref,
    /** Same as `patchPrefs`; named for the sliders' drag-end call sites. */
    persistPref: patchPrefs,
    resetFontPrefs,
    openUserSettingsDir,
  };
}

export type GeneralPrefsEditor = ReturnType<typeof useGeneralPrefsEditor>;
