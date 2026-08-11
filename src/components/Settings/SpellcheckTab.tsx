import { useCallback, useEffect, useState } from "react";
import {
  addCustomDictionaryWord,
  getCustomDictionaryWords,
  getDictionaries,
  getSpellcheckConfig,
  removeCustomDictionaryWord,
  setSpellcheckConfig,
  type DictionaryDef,
  type SpellcheckConfig,
} from "../../lib/spellcheck";
import "../Welcome/CloneRepoModal.css";
import "./SpellcheckTab.css";
import "./StandardsRulesTab.css";

type SpellcheckTabProps = {
  onConfigChange?: (config: SpellcheckConfig) => void;
};

export function SpellcheckTab({ onConfigChange }: SpellcheckTabProps) {
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
      setError(e instanceof Error ? e.message : String(e));
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
          setError(e instanceof Error ? e.message : String(e));
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
        setError(e instanceof Error ? e.message : String(e));
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
      setError(e instanceof Error ? e.message : String(e));
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
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    },
    [reloadWords],
  );

  return (
    <>
      <div className="settings-section-title">Проверка орфографии</div>
      <p className="settings-lead">
        Подсвечивает слова, написанные с ошибкой, прямо в редакторе, и
        предлагает исправление. Слово считается ошибкой, только если оно не
        найдено ни в одном из включённых словарей.
      </p>

      <div className="settings-row">
        <label className="settings-check">
          <input
            type="checkbox"
            checked={config?.enabled ?? true}
            disabled={!config || busy}
            onChange={(event) => toggleEnabled(event.target.checked)}
          />
          <span>Включить проверку орфографии</span>
        </label>
      </div>

      <div className="settings-row">
        <label className="settings-check">
          <input
            type="checkbox"
            checked={config?.skipCamelCase ?? true}
            disabled={!config || busy || !(config?.enabled ?? true)}
            onChange={(event) => toggleSkipCamelCase(event.target.checked)}
          />
          <span>Не проверять слова в camelCase (getUserInfo, isEnabled)</span>
        </label>
      </div>

      <div className="settings-row">
        <label className="settings-check">
          <input
            type="checkbox"
            checked={config?.checkTxt ?? false}
            disabled={!config || busy || !(config?.enabled ?? true)}
            onChange={(event) => toggleCheckTxt(event.target.checked)}
          />
          <span>Проверять файлы .txt</span>
        </label>
      </div>

      <div className="standards-rules-list">
        {dictionaries === null ? (
          <p className="settings-hint">Загрузка…</p>
        ) : (
          dictionaries.map((dict) => (
            <div key={dict.id} className="standards-rule-row">
              <label className="settings-check">
                <input
                  type="checkbox"
                  checked={isDictionaryEnabled(dict)}
                  disabled={!config || busy || !(config?.enabled ?? true)}
                  onChange={(event) =>
                    toggleDictionary(dict, event.target.checked)
                  }
                />
                <span>{dict.title}</span>
              </label>
            </div>
          ))
        )}
      </div>

      <div className="settings-section-title">Личный словарь</div>
      <p className="settings-lead">
        Слова, которые вы добавили как правильные (через быстрое исправление
        в редакторе или здесь). Хранится в{" "}
        <code>~/.atlas/dictionaries/custom.txt</code>.
      </p>

      <div className="settings-row">
        <div className="spellcheck-add-word">
          <input
            type="text"
            className="clone-modal-input"
            value={newWord}
            placeholder="Добавить слово…"
            disabled={busy}
            onChange={(event) => setNewWord(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void addWord();
            }}
          />
          <button
            type="button"
            className="settings-btn primary"
            disabled={busy || !newWord.trim()}
            onClick={() => void addWord()}
          >
            Добавить
          </button>
        </div>
      </div>

      <div className="standards-rules-list">
        {words === null ? (
          <p className="settings-hint">Загрузка…</p>
        ) : words.length === 0 ? (
          <p className="settings-hint">Словарь пуст.</p>
        ) : (
          words.map((word) => (
            <div key={word} className="standards-rule-row">
              <span>{word}</span>
              <button
                type="button"
                className="settings-btn"
                disabled={busy}
                onClick={() => void removeWord(word)}
              >
                Удалить
              </button>
            </div>
          ))
        )}
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </>
  );
}
