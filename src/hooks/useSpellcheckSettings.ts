import { useCallback, useEffect, useState } from "react";
import { toMessage } from "../lib/errors";
import {
  addCustomDictionaryWord,
  getCustomDictionaryWords,
  getDictionaries,
  getSpellcheckConfig,
  removeCustomDictionaryWord,
  setSpellcheckConfig,
  type DictionaryDef,
  type SpellcheckConfig,
} from "../lib/spellcheck";

/** The spellcheck settings tab's state and every action it can take.
 *
 * `onConfigChange` lets the surrounding dialog re-apply the config live
 * (the editor re-runs its check without waiting for a reopen) — it fires
 * only after the write actually lands.
 *
 * Config edits are optimistic: the toggle flips immediately, and a failed
 * write rolls back to whatever the backend really holds rather than leaving
 * the UI showing a setting that was never saved. */
export function useSpellcheckSettings(
  onConfigChange?: (config: SpellcheckConfig) => void,
) {
  const [dictionaries, setDictionaries] = useState<DictionaryDef[] | null>(
    null,
  );
  const [config, setConfig] = useState<SpellcheckConfig | null>(null);
  const [words, setWords] = useState<string[] | null>(null);
  const [newWord, setNewWord] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const reloadWords = useCallback(async () => {
    try {
      setWords(await getCustomDictionaryWords());
    } catch (e) {
      setError(toMessage(e));
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [nextDictionaries, nextConfig] = await Promise.all([
          getDictionaries(),
          getSpellcheckConfig(),
        ]);
        if (!cancelled) {
          setDictionaries(nextDictionaries);
          setConfig(nextConfig);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) {
          setError(toMessage(e));
        }
      }
    })();
    void reloadWords();
    return () => {
      cancelled = true;
    };
  }, [reloadWords]);

  const persistConfig = useCallback(
    async (next: SpellcheckConfig) => {
      setConfig(next);
      setBusy(true);
      try {
        await setSpellcheckConfig(next);
        onConfigChange?.(next);
        setError(null);
      } catch (e) {
        setError(toMessage(e));
        const current = await getSpellcheckConfig().catch(() => config);
        if (current) setConfig(current);
      } finally {
        setBusy(false);
      }
    },
    [config, onConfigChange],
  );

  const isDictionaryEnabled = useCallback(
    (dict: DictionaryDef) => config?.dictionaries[dict.id] ?? true,
    [config],
  );

  const toggleDictionary = useCallback(
    (dict: DictionaryDef, enabled: boolean) => {
      if (!config) return;
      void persistConfig({
        ...config,
        dictionaries: { ...config.dictionaries, [dict.id]: enabled },
      });
    },
    [config, persistConfig],
  );

  const toggleEnabled = useCallback(
    (enabled: boolean) => {
      if (!config) return;
      void persistConfig({ ...config, enabled });
    },
    [config, persistConfig],
  );

  const toggleSkipCamelCase = useCallback(
    (skipCamelCase: boolean) => {
      if (!config) return;
      void persistConfig({ ...config, skipCamelCase });
    },
    [config, persistConfig],
  );

  const toggleCheckTxt = useCallback(
    (checkTxt: boolean) => {
      if (!config) return;
      void persistConfig({ ...config, checkTxt });
    },
    [config, persistConfig],
  );

  const addWord = useCallback(async () => {
    const word = newWord.trim();
    if (!word) return;
    setBusy(true);
    try {
      await addCustomDictionaryWord(word);
      setNewWord("");
      await reloadWords();
      setError(null);
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setBusy(false);
    }
  }, [newWord, reloadWords]);

  const removeWord = useCallback(
    async (word: string) => {
      setBusy(true);
      try {
        await removeCustomDictionaryWord(word);
        await reloadWords();
        setError(null);
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [reloadWords],
  );


  return {
    dictionaries,
    config,
    words,
    newWord,
    setNewWord,
    error,
    busy,
    isDictionaryEnabled,
    toggleDictionary,
    toggleEnabled,
    toggleSkipCamelCase,
    toggleCheckTxt,
    addWord,
    removeWord,
  };
}
