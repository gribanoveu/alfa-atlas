import { useCallback, useEffect, useRef, useState } from "react";
import {
  getMasterKeyAccessStatus,
  retryMasterKeyAccess,
  MASTER_KEY_UNLOCKED_EVENT,
} from "../lib/masterKey";

/** Re-runs `refresh` when the master key becomes readable again. For panels
 * that decide at mount whether a credential exists — see
 * `MASTER_KEY_UNLOCKED_EVENT`. */
export function useRefreshOnMasterKeyUnlock(refresh: () => void) {
  useEffect(() => {
    window.addEventListener(MASTER_KEY_UNLOCKED_EVENT, refresh);
    return () => window.removeEventListener(MASTER_KEY_UNLOCKED_EVENT, refresh);
  }, [refresh]);
}

/** Whether the OS keychain master key is reachable, and the retry the user
 * can trigger when it is not.
 *
 * Checked on mount and after an explicit retry, never on a timer: a poll
 * that outlives the backend's failure cooldown reaches the keychain again,
 * which on macOS means an access prompt nobody asked for.
 *
 * `retryDenied` is what separates "we have not tried yet" from "we tried and
 * the OS said no again" — the two call for different words on screen. */
export function useMasterKeyAccess() {
  const [unreachable, setUnreachable] = useState(false);
  const [retryDenied, setRetryDenied] = useState(false);
  const [retryBusy, setRetryBusy] = useState(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      const next = await getMasterKeyAccessStatus();
      if (mounted.current) setUnreachable(next.unreachable);
    } catch (e) {
      // The status call itself failing is an IPC fault, not a keychain
      // verdict — reporting it as one would claim the keychain is locked for
      // an unrelated cause.
      console.error("master key access status failed", e);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const retry = useCallback(async () => {
    setRetryBusy(true);
    try {
      const next = await retryMasterKeyAccess();
      // Announced before the early return: the panels that need to hear it
      // are not this component, and whether this banner is still mounted
      // says nothing about whether they are.
      if (!next.unreachable) {
        window.dispatchEvent(new CustomEvent(MASTER_KEY_UNLOCKED_EVENT));
      }
      if (!mounted.current) return;
      setUnreachable(next.unreachable);
      setRetryDenied(next.unreachable);
    } catch (e) {
      console.error("master key access retry failed", e);
    } finally {
      if (mounted.current) setRetryBusy(false);
    }
  }, []);

  return { unreachable, retryBusy, retryDenied, retry, refresh };
}
