import { openPath } from "@tauri-apps/plugin-opener";
import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import {
  AUTOSAVE_DELAY_LIMITS,
  clampAutosaveDelayMs,
  clampFontSizePx,
  DEFAULT_GENERAL_PREFS,
  FONT_SIZE_LIMITS,
  getGeneralPrefs,
  getSettingsPaths,
  setGeneralPrefs,
  type ErrorLanguage,
  type GeneralPrefs,
  type SettingsPaths,
} from "../../lib/prefs";
import { SUPPORTED_FORMAT_LABELS } from "../../lib/supportedFiles";
import "../Welcome/CloneRepoModal.css";
import "./SettingsDialog.css";
import { CredentialsTab } from "./CredentialsTab";
import { StandardsRulesTab } from "./StandardsRulesTab";

export type SectionId =
  | "general"
  | "editor"
  | "formats"
  | "paths"
  | "credentials"
  | "standards";

const SECTIONS: { id: SectionId; label: string }[] = [
  { id: "general", label: "Общие" },
  { id: "editor", label: "Редактор" },
  { id: "formats", label: "Файлы" },
  { id: "paths", label: "Пути" },
  { id: "credentials", label: "Git" },
  { id: "standards", label: "Стандарты" },
];

type FontSizePrefKey =
  | "uiFontSizePx"
  | "sidebarFontSizePx"
  | "editorFontSizePx"
  | "previewFontSizePx";

const FONT_SIZE_FIELDS: {
  key: FontSizePrefKey;
  id: string;
  label: string;
  hint: string;
}[] = [
  {
    key: "uiFontSizePx",
    id: "font-ui-size",
    label: "Интерфейс",
    hint: "Панели, меню, статус",
  },
  {
    key: "sidebarFontSizePx",
    id: "font-sidebar-size",
    label: "Боковая панель",
    hint: "Дерево файлов",
  },
  {
    key: "editorFontSizePx",
    id: "font-editor-size",
    label: "Редактор",
    hint: "Monaco, diff",
  },
  {
    key: "previewFontSizePx",
    id: "font-preview-size",
    label: "Превью",
    hint: "AsciiDoc, Markdown, JSON/YAML",
  },
];

const ERROR_LANGUAGE_OPTIONS: { value: ErrorLanguage; label: string }[] = [
  { value: "ru", label: "Русский" },
  { value: "en", label: "English" },
];

type SettingsDialogProps = {
  projectRoot: string | null;
  onClose: () => void;
  onCloseProject?: () => Promise<void>;
  onPrefsChange?: (prefs: GeneralPrefs) => void;
  initialSection?: SectionId;
};

export function SettingsDialog({
  projectRoot,
  onClose,
  onPrefsChange,
  initialSection,
}: SettingsDialogProps) {
  const [section, setSection] = useState<SectionId>(initialSection ?? "general");
  const [prefs, setPrefs] = useState<GeneralPrefs | null>(null);
  const [paths, setPaths] = useState<SettingsPaths | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [langOpen, setLangOpen] = useState(false);
  const langRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [nextPrefs, nextPaths] = await Promise.all([
          getGeneralPrefs(),
          getSettingsPaths(),
        ]);
        if (!cancelled) {
          setPrefs(nextPrefs);
          setPaths(nextPaths);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) {
          setError(e instanceof Error ? e.message : String(e));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (langOpen) {
        setLangOpen(false);
        return;
      }
      onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [langOpen, onClose]);

  useEffect(() => {
    if (!langOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!langRef.current?.contains(event.target as Node)) {
        setLangOpen(false);
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [langOpen]);

  useEffect(() => {
    setLangOpen(false);
  }, [section]);

  const persistPrefs = useCallback(
    async (next: GeneralPrefs) => {
      setPrefs(next);
      setBusy(true);
      try {
        await setGeneralPrefs(next);
        onPrefsChange?.(next);
        setError(null);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        const current = await getGeneralPrefs().catch(() => prefs);
        if (current) setPrefs(current);
      } finally {
        setBusy(false);
      }
    },
    [onPrefsChange, prefs],
  );

  const patchPrefs = useCallback(
    (patch: Partial<GeneralPrefs>) => {
      if (!prefs) return;
      void persistPrefs({ ...prefs, ...patch });
    },
    [persistPrefs, prefs],
  );

  const stageFontPref = useCallback(
    (patch: Partial<GeneralPrefs>) => {
      if (!prefs) return;
      const next = { ...prefs, ...patch };
      setPrefs(next);
      onPrefsChange?.(next);
    },
    [onPrefsChange, prefs],
  );

  const persistFontPref = useCallback(
    (patch: Partial<GeneralPrefs>) => {
      if (!prefs) return;
      void persistPrefs({ ...prefs, ...patch });
    },
    [persistPrefs, prefs],
  );

  const resetFontPrefs = useCallback(() => {
    if (!prefs) return;
    void persistPrefs({
      ...prefs,
      uiFontSizePx: DEFAULT_GENERAL_PREFS.uiFontSizePx,
      sidebarFontSizePx: DEFAULT_GENERAL_PREFS.sidebarFontSizePx,
      editorFontSizePx: DEFAULT_GENERAL_PREFS.editorFontSizePx,
      previewFontSizePx: DEFAULT_GENERAL_PREFS.previewFontSizePx,
    });
  }, [persistPrefs, prefs]);

  const openUserSettingsDir = useCallback(async () => {
    if (!paths?.userSettingsDir) return;
    try {
      await openPath(paths.userSettingsDir);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [paths]);

  return (
    <div
      className="settings-backdrop"
      role="presentation"
      onClick={onClose}
    >
      <div
        className="settings-dialog"
        role="dialog"
        aria-labelledby="settings-dialog-title"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="settings-header">
          <h2 className="settings-title" id="settings-dialog-title">
            Настройки
          </h2>
          <button
            type="button"
            className="settings-close"
            onClick={onClose}
            aria-label="Закрыть"
          >
            ×
          </button>
        </header>

        <div className="settings-body">
          <nav className="settings-nav" aria-label="Разделы настроек">
            {SECTIONS.map((item) => (
              <button
                key={item.id}
                type="button"
                className={`settings-nav-btn${section === item.id ? " active" : ""}`}
                onClick={() => setSection(item.id)}
              >
                {item.label}
              </button>
            ))}
          </nav>

          <div className="settings-content">
            {section === "general" ? (
              <>
                <div className="settings-section-title">Общие</div>
                <div className="settings-row">
                  <label className="settings-check">
                    <input
                      type="checkbox"
                      checked={prefs?.restoreLastProject ?? true}
                      disabled={!prefs || busy}
                      onChange={(event) =>
                        patchPrefs({ restoreLastProject: event.target.checked })
                      }
                    />
                    <span>Открывать последний проект при запуске</span>
                  </label>
                  <p className="settings-hint">
                    Если выключено, приложение стартует с экрана Welcome, даже
                    если путь к проекту сохранён.
                  </p>
                </div>
                <div className="settings-row">
                  <span
                    className="settings-field-label"
                    id="error-language-label"
                  >
                    Язык сообщений об ошибках
                  </span>
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
                          {ERROR_LANGUAGE_OPTIONS.find(
                            (o) => o.value === (prefs?.errorLanguage ?? "ru"),
                          )?.label ?? "Русский"}
                        </span>
                      </span>
                      <span className="clone-select-chevron" aria-hidden>
                        ▾
                      </span>
                    </button>
                    {langOpen ? (
                      <div className="clone-select-menu" role="listbox">
                        {ERROR_LANGUAGE_OPTIONS.map((option) => {
                          const active =
                            option.value === (prefs?.errorLanguage ?? "ru");
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
                              <span className="clone-select-path">
                                {option.label}
                              </span>
                            </button>
                          );
                        })}
                      </div>
                    ) : null}
                  </div>
                  <p className="settings-hint">
                    Язык текста диагностик в панели «Проблемы» (битые include,
                    xref, изображения, циклы и т.п.). Применяется при следующей
                    переиндексации.
                  </p>
                </div>
                <div className="settings-row">
                  <div className="settings-section-head">
                    <div className="settings-section-title">Шрифты</div>
                    <button
                      type="button"
                      className="settings-link-btn"
                      disabled={!prefs || busy}
                      onClick={resetFontPrefs}
                    >
                      Сбросить
                    </button>
                  </div>
                  <div
                    className="settings-font-grid"
                    role="group"
                    aria-label="Размер шрифта по зонам"
                  >
                    <span className="settings-font-grid-head">Зона</span>
                    <span className="settings-font-grid-head">px</span>
                    {FONT_SIZE_FIELDS.map(({ key, id, label, hint }) => (
                      <Fragment key={key}>
                        <label className="settings-font-label" htmlFor={id}>
                          <span className="settings-font-name">{label}</span>
                          <span className="settings-font-desc">{hint}</span>
                        </label>
                        <input
                          id={id}
                          className="settings-number settings-font-input"
                          type="number"
                          min={FONT_SIZE_LIMITS.min}
                          max={FONT_SIZE_LIMITS.max}
                          step={FONT_SIZE_LIMITS.step}
                          value={prefs?.[key] ?? DEFAULT_GENERAL_PREFS[key]}
                          disabled={!prefs || busy}
                          onChange={(event) => {
                            if (!prefs) return;
                            const raw = Number(event.target.value);
                            if (!Number.isFinite(raw)) return;
                            stageFontPref({
                              [key]: clampFontSizePx(raw),
                            } as Pick<GeneralPrefs, FontSizePrefKey>);
                          }}
                          onBlur={() => {
                            if (!prefs) return;
                            void persistFontPref({
                              [key]: clampFontSizePx(prefs[key]),
                            } as Pick<GeneralPrefs, FontSizePrefKey>);
                          }}
                        />
                      </Fragment>
                    ))}
                  </div>
                  <p className="settings-hint settings-hint-compact">
                    {FONT_SIZE_LIMITS.min}–{FONT_SIZE_LIMITS.max} px, шаг{" "}
                    {FONT_SIZE_LIMITS.step}. Изменения видны сразу, сохранение —
                    при потере фокуса.
                  </p>
                </div>
              </>
            ) : null}

            {section === "editor" ? (
              <>
                <div className="settings-section-title">Автосохранение</div>
                <div className="settings-row">
                  <label className="settings-check">
                    <input
                      type="checkbox"
                      checked={prefs?.autosaveEnabled ?? true}
                      disabled={!prefs || busy}
                      onChange={(event) =>
                        patchPrefs({ autosaveEnabled: event.target.checked })
                      }
                    />
                    <span>Автосохранение при редактировании</span>
                  </label>
                  <p className="settings-hint">
                    Периодически записывает файл на диск после паузы ввода.
                  </p>
                </div>
                <div className="settings-row">
                  <label className="settings-check">
                    <input
                      type="checkbox"
                      checked={prefs?.saveOnTabSwitch ?? true}
                      disabled={!prefs || busy}
                      onChange={(event) =>
                        patchPrefs({ saveOnTabSwitch: event.target.checked })
                      }
                    />
                    <span>Сохранять при переключении вкладки</span>
                  </label>
                </div>
                <div className="settings-row">
                  <label className="settings-field-label" htmlFor="autosave-delay">
                    Задержка автосохранения (мс)
                  </label>
                  <input
                    id="autosave-delay"
                    className="settings-number"
                    type="number"
                    min={AUTOSAVE_DELAY_LIMITS.min}
                    max={AUTOSAVE_DELAY_LIMITS.max}
                    step={100}
                    value={prefs?.autosaveDelayMs ?? 1000}
                    disabled={!prefs || busy || !(prefs?.autosaveEnabled ?? true)}
                    onChange={(event) => {
                      if (!prefs) return;
                      const raw = Number(event.target.value);
                      if (!Number.isFinite(raw)) return;
                      setPrefs({
                        ...prefs,
                        autosaveDelayMs: clampAutosaveDelayMs(raw),
                      });
                    }}
                    onBlur={() => {
                      if (!prefs) return;
                      void persistPrefs({
                        ...prefs,
                        autosaveDelayMs: clampAutosaveDelayMs(
                          prefs.autosaveDelayMs,
                        ),
                      });
                    }}
                  />
                  <p className="settings-hint">
                    От {AUTOSAVE_DELAY_LIMITS.min} до{" "}
                    {AUTOSAVE_DELAY_LIMITS.max} мс.
                  </p>
                </div>
                <div className="settings-row">
                  <div className="settings-section-title">Проводник</div>
                  <label className="settings-check">
                    <input
                      type="checkbox"
                      checked={prefs?.separateExternalFolder ?? true}
                      disabled={!prefs || busy}
                      onChange={(event) =>
                        patchPrefs({
                          separateExternalFolder: event.target.checked,
                        })
                      }
                    />
                    <span>Отдельно показывать папку _external</span>
                  </label>
                  <p className="settings-hint">
                    Если в корне документации есть папка _external, она
                    отображается отдельным блоком под основным деревом.
                  </p>
                </div>
              </>
            ) : null}

            {section === "formats" ? (
              <>
                <div className="settings-section-title">Поддерживаемые форматы</div>
                <p className="settings-lead">
                  Alfa Atlas открывает и редактирует файлы со следующими
                  расширениями.
                </p>
                <div className="settings-formats">
                  {SUPPORTED_FORMAT_LABELS.map((label) => (
                    <span key={label} className="settings-format-chip">
                      {label}
                    </span>
                  ))}
                </div>
              </>
            ) : null}

            {section === "paths" ? (
              <>
                <div className="settings-section-title">Пути</div>
                <div className="settings-row">
                  <span className="settings-hint" style={{ paddingLeft: 0 }}>
                    Папка настроек пользователя (~/.atlas)
                  </span>
                  <div className="settings-path">
                    {paths?.userSettingsDir ?? "…"}
                  </div>
                </div>
                <div className="settings-row">
                  <span className="settings-hint" style={{ paddingLeft: 0 }}>
                    Текущий / сохранённый проект
                  </span>
                  <div
                    className={`settings-path${!paths?.projectRoot && !projectRoot ? " empty" : ""}`}
                  >
                    {projectRoot ?? paths?.projectRoot ?? "Проект не открыт"}
                  </div>
                </div>
                <div className="settings-row">
                  <span className="settings-hint" style={{ paddingLeft: 0 }}>
                    Настройки проекта (.atlas)
                  </span>
                  <div
                    className={`settings-path${!paths?.projectConfigDir && !projectRoot ? " empty" : ""}`}
                  >
                    {projectRoot
                      ? `${projectRoot.replace(/[/\\]+$/, "")}/.atlas`
                      : (paths?.projectConfigDir ?? "—")}
                  </div>
                </div>
                <div className="settings-actions">
                  <button
                    type="button"
                    className="settings-btn primary"
                    disabled={!paths?.userSettingsDir}
                    onClick={() => void openUserSettingsDir()}
                  >
                    Открыть папку настроек
                  </button>
                </div>
              </>
            ) : null}

            {section === "credentials" ? <CredentialsTab /> : null}

            {section === "standards" ? <StandardsRulesTab /> : null}

            {error ? <div className="settings-error">{error}</div> : null}
          </div>
        </div>

        <footer className="settings-footer">
          <button type="button" className="settings-btn" onClick={onClose}>
            Закрыть
          </button>
        </footer>
      </div>
    </div>
  );
}
