import { useSpellcheckSettings } from "../../hooks/useSpellcheckSettings";
import type { SpellcheckConfig } from "../../lib/spellcheck";
import "../Welcome/CloneRepoModal.css";
import "./SpellcheckTab.css";
import "./StandardsRulesTab.css";

type SpellcheckTabProps = {
  onConfigChange?: (config: SpellcheckConfig) => void;
};

export function SpellcheckTab({ onConfigChange }: SpellcheckTabProps) {
  const {
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
  } = useSpellcheckSettings(onConfigChange);

  return (
    <div className="settings-sections">
      <div className="settings-card">
        <div className="settings-section-title">Проверка орфографии</div>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={config?.enabled ?? true}
            disabled={!config || busy}
            onChange={(event) => toggleEnabled(event.target.checked)}
          />
          <span>Включить проверку орфографии</span>
        </label>

        <hr className="settings-card-divider" />

        <label className="settings-check">
          <input
            type="checkbox"
            checked={config?.skipCamelCase ?? true}
            disabled={!config || busy || !(config?.enabled ?? true)}
            onChange={(event) => toggleSkipCamelCase(event.target.checked)}
          />
          <span>Не проверять слова в camelCase (getUserInfo, isEnabled)</span>
        </label>

        <hr className="settings-card-divider" />

        <label className="settings-check">
          <input
            type="checkbox"
            checked={config?.checkTxt ?? false}
            disabled={!config || busy || !(config?.enabled ?? true)}
            onChange={(event) => toggleCheckTxt(event.target.checked)}
          />
          <span>Проверять файлы .txt (может замедлять отклик редактора при открытии файлов без связанного текста, таких фалов как конфигурационные файлы и подобное)</span>
        </label>
      </div>

      <div className="settings-card">
        <div className="settings-section-title">Словари</div>
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
      </div>

      <div className="settings-card">
        <div className="settings-section-title">Личный словарь</div>
        <p className="settings-hint settings-hint-compact">
          Слова, которые вы добавили как правильные (через быстрое исправление
          в редакторе или здесь). Хранится в{" "}
          <code>~/.atlas/dictionaries/custom.txt</code>.
        </p>

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

        <div className="standards-rules-list">
          {words === null ? (
            <p className="settings-hint">Загрузка…</p>
          ) : words.length === 0 ? (
            <p className="settings-hint">Словарь пуст.</p>
          ) : (
            words.map((word) => (
              <div key={word} className="standards-rule-row">
                <span className="spellcheck-word">{word}</span>
                <button
                  type="button"
                  className="settings-link-btn danger"
                  disabled={busy}
                  onClick={() => void removeWord(word)}
                >
                  Удалить
                </button>
              </div>
            ))
          )}
        </div>
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
