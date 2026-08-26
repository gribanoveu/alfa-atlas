import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import {
  gitGenerateKey,
  gitGetCredentials,
  gitGetKeyStatus,
  gitImportKey,
  gitSaveCredentials,
  type AppKeyStatus,
  type GitCredentials,
  type SshKeyConfig,
} from "../lib/git";

/** Git credentials and the app's own SSH key, plus everything the settings
 * tab can do to them.
 *
 * Dialog state (which modal is open, which key row is being edited) stays
 * with the component — it is presentation, and keeping it out means
 * `saveKey` takes the index it should write to rather than the hook having
 * to track what the UI is currently showing. */
export function useGitCredentials() {
  const [credentials, setCredentials] = useState<GitCredentials | null>(null);
  const [keyStatus, setKeyStatus] = useState<AppKeyStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [keyGenBusy, setKeyGenBusy] = useState(false);
  const [copyFeedback, setCopyFeedback] = useState(false);
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
        const [creds, status] = await Promise.all([
          gitGetCredentials(),
          gitGetKeyStatus(),
        ]);
        if (!mounted.current) return;
        setCredentials(creds);
        setKeyStatus(status);
        setError(null);
      } catch (e) {
        if (mounted.current) setError(toMessage(e));
      }
    })();
  }, []);

  /** Optimistic: the UI shows the new value immediately, and a failed write
   * rolls back to whatever the backend actually holds rather than leaving a
   * setting displayed that was never saved. */
  const persist = useCallback(async (next: GitCredentials) => {
    setCredentials(next);
    setBusy(true);
    try {
      await gitSaveCredentials(next);
      if (mounted.current) setError(null);
    } catch (e) {
      if (!mounted.current) return;
      setError(toMessage(e));
      const current = await gitGetCredentials().catch(() => null);
      if (current && mounted.current) setCredentials(current);
    } finally {
      if (mounted.current) setBusy(false);
    }
  }, []);

  const toggleTrustAll = useCallback(() => {
    if (!credentials) return;
    void persist({
      ...credentials,
      trustAllSshHostKeys: !credentials.trustAllSshHostKeys,
    });
  }, [credentials, persist]);

  const deleteKey = useCallback(
    (index: number) => {
      if (!credentials) return;
      const sshKeys = [...credentials.sshKeys];
      sshKeys.splice(index, 1);
      void persist({ ...credentials, sshKeys });
    },
    [credentials, persist],
  );

  /** `at` is the row being edited, or `null` to append a new key. */
  const saveKey = useCallback(
    (config: SshKeyConfig, at: number | null) => {
      if (!credentials) return;
      if (at === null) {
        void persist({ ...credentials, sshKeys: [...credentials.sshKeys, config] });
        return;
      }
      const sshKeys = [...credentials.sshKeys];
      sshKeys[at] = config;
      void persist({ ...credentials, sshKeys });
    },
    [credentials, persist],
  );

  const runKeyOp = useCallback(async (op: () => Promise<AppKeyStatus>) => {
    setKeyGenBusy(true);
    setError(null);
    try {
      const status = await op();
      if (mounted.current) setKeyStatus(status);
    } catch (e) {
      if (mounted.current) setError(toMessage(e));
    } finally {
      if (mounted.current) setKeyGenBusy(false);
    }
  }, []);

  const generateKey = useCallback(() => runKeyOp(gitGenerateKey), [runKeyOp]);

  const importKey = useCallback(async () => {
    let selected: string | string[] | null;
    try {
      selected = await open({
        multiple: false,
        title: "Выберите файл приватного SSH ключа",
      });
    } catch {
      // Dialog cancelled or unavailable — not an error worth showing.
      return;
    }
    if (selected === null || Array.isArray(selected)) return;
    await runKeyOp(() => gitImportKey(selected as string));
  }, [runKeyOp]);

  /** Flashes "скопировано" for two seconds. A clipboard the OS refuses is
   * silently ignored — there is nothing the user could do about it. */
  const copyPublicKey = useCallback(async () => {
    if (!keyStatus?.publicKey) return;
    try {
      await navigator.clipboard.writeText(keyStatus.publicKey);
      if (!mounted.current) return;
      setCopyFeedback(true);
      setTimeout(() => {
        if (mounted.current) setCopyFeedback(false);
      }, 2000);
    } catch {
      // clipboard not available
    }
  }, [keyStatus?.publicKey]);

  return {
    credentials,
    keyStatus,
    error,
    busy,
    keyGenBusy,
    copyFeedback,
    toggleTrustAll,
    deleteKey,
    saveKey,
    generateKey,
    importKey,
    copyPublicKey,
  };
}
