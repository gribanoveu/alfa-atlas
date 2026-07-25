import { useCallback, useEffect, useState } from "react";
import {
  DEFAULT_GENERAL_PREFS,
  getGeneralPrefs,
  type GeneralPrefs,
} from "../lib/prefs";

export function useGeneralPrefs() {
  const [prefs, setPrefs] = useState<GeneralPrefs>(DEFAULT_GENERAL_PREFS);
  const [ready, setReady] = useState(false);

  const reload = useCallback(async () => {
    try {
      const next = await getGeneralPrefs();
      setPrefs({ ...DEFAULT_GENERAL_PREFS, ...next });
    } catch {
      setPrefs(DEFAULT_GENERAL_PREFS);
    } finally {
      setReady(true);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  return { prefs, ready, reload, setPrefs };
}
