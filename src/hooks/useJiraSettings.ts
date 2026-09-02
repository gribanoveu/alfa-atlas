import { useCallback, useEffect, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import {
  deleteJiraToken,
  getJiraSettings,
  jiraCurrentUser,
  jiraHasToken,
  setJiraSettings,
  setJiraToken,
  type JiraSettings,
  type JiraSettingsView,
  type JiraUser,
} from "../lib/jira";

export type JiraTestResult = { ok: true; user: JiraUser } | { ok: false; message: string };

/** State and actions behind the Jira settings tab.
 *
 * The token is deliberately never held here as a value read back from the
 * backend — `tokenSet` is all the backend will say. `tokenDraft` is what the
 * user is currently typing, and it is cleared the moment the write lands so
 * the secret doesn't linger in component state for the rest of the session.
 *
 * The form edits the *user* layer only (`view.settings`); `view.bundledBaseUrl`
 * / `view.hasBundledCert` are what the build supplies underneath and are
 * rendered as hints, never copied into the fields — copying them in would
 * silently turn a build default into a user override.
 *
 * Text fields edit locally and are written on `commit()` (blur), not per
 * keystroke — `settings.json` is a whole-file rewrite, and a URL being typed
 * is momentarily invalid anyway. A failed write rolls the form back to
 * whatever the backend really holds. */
export function useJiraSettings() {
  const [view, setView] = useState<JiraSettingsView | null>(null);
  const [tokenSet, setTokenSet] = useState(false);
  const [tokenDraft, setTokenDraft] = useState("");
  const [tokenSaved, setTokenSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<JiraTestResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  // What `commit()` should write. Kept in a ref as well as in state so a
  // blur that immediately follows a keystroke doesn't write the previous
  // render's value.
  const draft = useRef<JiraSettings | null>(null);
  // What the backend last confirmed, so `commit()` can skip a no-op write
  // (every blur on an untouched field would otherwise rewrite the file).
  const saved = useRef<JiraSettings | null>(null);

  const adopt = useCallback((next: JiraSettingsView) => {
    draft.current = next.settings;
    saved.current = next.settings;
    setView(next);
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [nextView, hasToken] = await Promise.all([getJiraSettings(), jiraHasToken()]);
        if (cancelled) return;
        adopt(nextView);
        setTokenSet(hasToken);
        setError(null);
      } catch (e) {
        if (!cancelled) setError(toMessage(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [adopt]);

  /** Local-only edit; nothing is written until `commit()`. */
  const setField = useCallback((patch: Partial<JiraSettings>) => {
    const current = draft.current;
    if (!current) return;
    const next = { ...current, ...patch };
    draft.current = next;
    setView((previous) => (previous ? { ...previous, settings: next } : previous));
  }, []);

  /** `patch` is for choices with no intermediate state — picking a project
   *  from a list is one act, not typing that later settles on blur. */
  const commit = useCallback(async (patch?: Partial<JiraSettings>) => {
    if (patch) setField(patch);
    const next = draft.current;
    if (!next) return;
    if (saved.current && shallowEqual(saved.current, next)) return;

    setBusy(true);
    try {
      await setJiraSettings(next);
      // The backend normalizes (trims the URL, drops a blank cert), so the
      // form shows what was actually stored rather than what was typed.
      adopt(await getJiraSettings());
      // A changed address or certificate invalidates whatever the last check
      // proved — «Соединение OK» next to new settings would be a lie.
      setTestResult(null);
      setError(null);
    } catch (e) {
      setError(toMessage(e));
      const current = await getJiraSettings().catch(() => null);
      if (current) adopt(current);
    } finally {
      setBusy(false);
    }
  }, [adopt, setField]);

  /** Вызывается по потере фокуса и по Enter, а не только кнопкой: токен,
   * набранный и оставленный в поле, иначе молча пропадал. Пустая строка
   * отсекается здесь же, поэтому уход с нетронутого поля ничего не пишет, а
   * клик по кнопке не сохраняет дважды — blur успевает очистить поле, и к
   * моменту клика кнопка уже неактивна. */
  const saveToken = useCallback(async () => {
    const token = tokenDraft.trim();
    if (!token) return;
    setBusy(true);
    try {
      await setJiraToken(token);
      setTokenSet(true);
      setTokenDraft("");
      setTestResult(null);
      setTokenSaved(true);
      setError(null);
      window.setTimeout(() => setTokenSaved(false), 1500);
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setBusy(false);
    }
  }, [tokenDraft]);

  const removeToken = useCallback(async () => {
    setBusy(true);
    try {
      await deleteJiraToken();
      setTokenSet(false);
      setTokenDraft("");
      setTestResult(null);
      setError(null);
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setBusy(false);
    }
  }, []);

  /** Writes any pending edit first, so the check runs against what the
   * backend holds rather than what the form shows. */
  const testConnection = useCallback(async () => {
    await commit();
    setTesting(true);
    setTestResult(null);
    try {
      const user = await jiraCurrentUser();
      setTestResult({ ok: true, user });
    } catch (e) {
      setTestResult({ ok: false, message: toMessage(e) });
    } finally {
      setTesting(false);
    }
  }, [commit]);

  return {
    view,
    tokenSet,
    tokenDraft,
    setTokenDraft,
    tokenSaved,
    busy,
    testing,
    testResult,
    error,
    setField,
    commit,
    saveToken,
    removeToken,
    testConnection,
  };
}

function shallowEqual(a: JiraSettings, b: JiraSettings): boolean {
  return (
    a.baseUrl === b.baseUrl &&
    a.projectKey === b.projectKey &&
    a.projectName === b.projectName &&
    a.issueTypeId === b.issueTypeId &&
    a.issueTypeName === b.issueTypeName &&
    a.trustedCertPem === b.trustedCertPem
  );
}
