import { useCallback, useEffect, useState } from "react";

import {
  getMetricsStatus,
  setMetricsEnabled,
  type MetricsStatus,
} from "../lib/metrics";
import { toMessage } from "../lib/errors";

export function useMetricsSettings() {
  const [status, setStatus] = useState<MetricsStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setStatus(await getMetricsStatus());
      setError(null);
    } catch (e) {
      setError(toMessage(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const setEnabled = useCallback(async (enabled: boolean) => {
    setBusy(true);
    try {
      setStatus(await setMetricsEnabled(enabled));
      setError(null);
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setBusy(false);
    }
  }, []);

  return { status, busy, error, setEnabled, reload };
}
