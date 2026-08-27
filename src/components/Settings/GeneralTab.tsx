import { useEffect, useRef, useState } from "react";
import type { GeneralPrefsEditor } from "../../hooks/useGeneralPrefsEditor";
import type { ErrorLanguage } from "../../lib/prefs";
import "../Welcome/CloneRepoModal.css";

const ERROR_LANGUAGE_OPTIONS: { value: ErrorLanguage; label: string }[] = [
  { value: "ru", label: "Русский" },
  { value: "en", label: "English" },
];

type GeneralTabProps = {
  editor: GeneralPrefsEditor;
};

/** Application-level preferences: what happens on startup, and the language
 * of diagnostics. Font sizes live in `AppearanceTab`, editing behaviour in
 * `EditorTab` — this tab stays about the app itself. */
export function GeneralTab({ editor }: GeneralTabProps) {
  const { prefs, error, busy, patchPrefs } = editor;
  const [langOpen, setLangOpen] = useState(false);
  const langRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!langOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!langRef.current?.contains(event.target as Node)) setLangOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      setLangOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [langOpen]);

  const currentLanguage = prefs?.errorLanguage ?? "ru";

  return (
    <div className="settings-sections">
      <div className="settings-card">
        <div className="settings-section-title">Запуск</div>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={prefs?.restoreLastProject ?? true}
            disabled={!prefs || busy}
            onChange={(event) => patchPrefs({ restoreLastProject: event.target.checked })}
          />
          <span>Открывать последний проект при запуске</span>
        </label>
        <p className="settings-hint">
          Если выключено, приложение стартует с экрана Welcome, даже если путь к
          проекту сохранён.
        </p>
      </div>

      <div className="settings-card">
        <div className="settings-section-title">Язык диагностик</div>
        <div className="clone-select settings-lang-select" ref={langRef}>
          <button
            type="button"
            id="error-language"
            className={`clone-select-trigger${langOpen ? " is-open" : ""}`}
            aria-haspopup="listbox"
            aria-expanded={langOpen}
            aria-labelledby="error-language-label"
            disabled={!prefs || busy}
            onClick={() => setLangOpen((open) => !open)}
          >
            <span className="clone-select-value">
              <span className="clone-select-path">
                {ERROR_LANGUAGE_OPTIONS.find((o) => o.value === currentLanguage)?.label ??
                  "Русский"}
              </span>
            </span>
            <span className="clone-select-chevron" aria-hidden>
              ▾
            </span>
          </button>
          {langOpen ? (
            <div className="clone-select-menu" role="listbox">
              {ERROR_LANGUAGE_OPTIONS.map((option) => {
                const active = option.value === currentLanguage;
                return (
                  <button
                    key={option.value}
                    type="button"
                    role="option"
                    aria-selected={active}
                    className={`clone-select-option${active ? " is-active" : ""}`}
                    onClick={() => {
                      patchPrefs({ errorLanguage: option.value });
                      setLangOpen(false);
                    }}
                  >
                    <span className="clone-select-path">{option.label}</span>
                  </button>
                );
              })}
            </div>
          ) : null}
        </div>
        <span className="settings-field-label settings-sr-only" id="error-language-label">
          Язык сообщений об ошибках
        </span>
        <p className="settings-hint settings-hint-compact">
          Язык текста диагностик в панели «Проблемы» (битые include, xref,
          изображения, циклы и т.п.). Применяется при следующей переиндексации.
        </p>
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
