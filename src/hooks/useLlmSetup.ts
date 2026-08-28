import { useCallback, useEffect, useState } from "react";
import {
  getLlmSettings,
  hasLlmApiKey,
  listLlmModels,
  listLlmProviders,
  removeLlmProvider,
  setLlmApiKey,
  setLlmSettings,
  testLlmConnection,
  upsertLlmProvider,
  type LlmModelInfo,
  type LlmProviderConfig,
  type LlmSettings,
  type ResolvedLlmProvider,
} from "../lib/llm";
import { toMessage } from "../lib/errors";

const llmSetupChangeListeners = new Set<() => void>();

/** Notifies every mounted `useLlmSetup` instance to re-fetch from the
 * backend. Settings, the assistant panel, and `App.tsx` each hold their
 * own hook instance (they're not visible simultaneously, but a change
 * made in Settings must reach the chat panel once it closes). */
function notifyLlmSetupChanged() {
  for (const listener of llmSetupChangeListeners) listener();
}

/**
 * LLM provider registry state, in one place — mirrors `useEmbeddingSetup`'s
 * shape (fetch-on-mount `refresh`, optimistic per-field updates, a
 * separate write-only API-key setter), adapted for a *list* of providers
 * (system presets + custom) rather than one global provider choice.
 *
 * Multiple components mount this hook independently; successful mutations
 * broadcast via `notifyLlmSetupChanged` so their copies stay aligned.
 */
export function useLlmSetup() {
  const [settings, setSettingsState] = useState<LlmSettings | null>(null);
  const [providers, setProviders] = useState<ResolvedLlmProvider[]>([]);
  const [hasApiKeyMap, setHasApiKeyMap] = useState<Record<string, boolean>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextSettings, nextProviders] = await Promise.all([
        getLlmSettings(),
        listLlmProviders(),
      ]);
      setSettingsState(nextSettings);
      setProviders(nextProviders);
      const keyChecks = await Promise.all(
        nextProviders.map(async (p): Promise<[string, boolean]> => [p.id, await hasLlmApiKey(p.id)]),
      );
      setHasApiKeyMap(Object.fromEntries(keyChecks));
      setError(null);
    } catch (e) {
      setError(toMessage(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const onChange = () => {
      void refresh();
    };
    llmSetupChangeListeners.add(onChange);
    return () => {
      llmSetupChangeListeners.delete(onChange);
    };
  }, [refresh]);

  const selectActiveProvider = useCallback(
    async (providerId: string) => {
      if (!settings) return;
      const next = { ...settings, activeProviderId: providerId };
      setSettingsState(next);
      setBusy(true);
      try {
        await setLlmSettings(next);
        setError(null);
        notifyLlmSetupChanged();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [settings],
  );

  /** Merges `patch` into whichever settings-layer entry (if any) already
   * exists for `providerId`, then upserts the result — for a system
   * provider id this is an override (unset fields stay `null`, meaning
   * "inherit from the manifest"); for a new custom id this builds the
   * entry up field by field, same as `useEmbeddingSetup`'s `updateConfig`
   * merges onto the single existing config. */
  const updateProviderConfig = useCallback(
    async (providerId: string, patch: Partial<Omit<LlmProviderConfig, "id">>) => {
      const existing = settings?.providers.find((p) => p.id === providerId);
      const next: LlmProviderConfig = {
        id: providerId,
        label: existing?.label ?? null,
        baseUrl: existing?.baseUrl ?? null,
        model: existing?.model ?? null,
        trustedCertPem: existing?.trustedCertPem ?? null,
        limit: existing?.limit ?? null,
        ...patch,
      };
      setBusy(true);
      try {
        await upsertLlmProvider(next);
        await refresh();
        setError(null);
        notifyLlmSetupChanged();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [settings, refresh],
  );

  const setDebugLogging = useCallback(
    async (enabled: boolean) => {
      if (!settings) return;
      const next = { ...settings, debugLogging: enabled };
      setSettingsState(next);
      setBusy(true);
      try {
        await setLlmSettings(next);
        setError(null);
        notifyLlmSetupChanged();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [settings],
  );

  const setFollowUpSuggestionsDisabled = useCallback(
    async (disabled: boolean) => {
      if (!settings) return;
      const next = { ...settings, followUpSuggestionsDisabled: disabled };
      setSettingsState(next);
      setBusy(true);
      try {
        await setLlmSettings(next);
        setError(null);
        notifyLlmSetupChanged();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [settings],
  );

  const setToolCallLogging = useCallback(
    async (enabled: boolean) => {
      if (!settings) return;
      const next = { ...settings, toolCallLogging: enabled };
      setSettingsState(next);
      setBusy(true);
      try {
        await setLlmSettings(next);
        setError(null);
        notifyLlmSetupChanged();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [settings],
  );

  const setTaskDoneSoundEnabled = useCallback(
    async (enabled: boolean) => {
      if (!settings) return;
      const next = { ...settings, taskDoneSoundEnabled: enabled };
      setSettingsState(next);
      setBusy(true);
      try {
        await setLlmSettings(next);
        setError(null);
        notifyLlmSetupChanged();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [settings],
  );

  const setNeedAnswerSoundEnabled = useCallback(
    async (enabled: boolean) => {
      if (!settings) return;
      const next = { ...settings, needAnswerSoundEnabled: enabled };
      setSettingsState(next);
      setBusy(true);
      try {
        await setLlmSettings(next);
        setError(null);
        notifyLlmSetupChanged();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [settings],
  );

  const setRateLimitEnabled = useCallback(
    async (enabled: boolean) => {
      if (!settings) return;
      const next = { ...settings, rateLimitEnabled: enabled };
      setSettingsState(next);
      setBusy(true);
      try {
        await setLlmSettings(next);
        setError(null);
        notifyLlmSetupChanged();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [settings],
  );

  const setMemoryExtractionEnabled = useCallback(
    async (enabled: boolean) => {
      if (!settings) return;
      const next = { ...settings, memoryExtractionEnabled: enabled };
      setSettingsState(next);
      setBusy(true);
      try {
        await setLlmSettings(next);
        setError(null);
        notifyLlmSetupChanged();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [settings],
  );

  const removeProvider = useCallback(
    async (providerId: string) => {
      setBusy(true);
      try {
        await removeLlmProvider(providerId);
        await refresh();
        setError(null);
        notifyLlmSetupChanged();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  const saveApiKey = useCallback(async (providerId: string, apiKey: string) => {
    setBusy(true);
    try {
      await setLlmApiKey(providerId, apiKey);
      setHasApiKeyMap((prev) => ({ ...prev, [providerId]: true }));
      setError(null);
      notifyLlmSetupChanged();
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setBusy(false);
    }
  }, []);

  /** On-demand only (needs network + a saved key) — never fetched eagerly
   * for every provider in `refresh`. */
  const loadModels = useCallback((providerId: string): Promise<LlmModelInfo[]> => {
    return listLlmModels(providerId);
  }, []);

  const testConnection = useCallback((providerId: string): Promise<string> => {
    return testLlmConnection(providerId);
  }, []);

  return {
    settings,
    providers,
    hasApiKeyMap,
    busy,
    error,
    refresh,
    selectActiveProvider,
    updateProviderConfig,
    setDebugLogging,
    setFollowUpSuggestionsDisabled,
    setToolCallLogging,
    setTaskDoneSoundEnabled,
    setNeedAnswerSoundEnabled,
    setRateLimitEnabled,
    setMemoryExtractionEnabled,
    removeProvider,
    saveApiKey,
    loadModels,
    testConnection,
  };
}
