import { openPath } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useState } from "react";
import {
  AUTOSAVE_DELAY_LIMITS,
  clampAutosaveDelayMs,
  getGeneralPrefs,
  getSettingsPaths,
  setGeneralPrefs,
  type GeneralPrefs,
  type SettingsPaths,
} from "../../lib/prefs";
import { SUPPORTED_FORMAT_LABELS } from "../../lib/supportedFiles";
import "./SettingsDialog.css";

type SectionId = "general" | "editor" | "paths";

const SECTIONS: { id: SectionId; label: string }[] = [
  { id: "general", label: "Общие" },
  { id: "editor", label: "Редактор" },
  { id: "paths", label: "Пути" },
];

type SettingsDialogProps = {
  projectRoot: string | null;
  onClose: () => void;
  onCloseProject?: () => Promise<void>;
  onPrefsChange?: (prefs: GeneralPrefs) => void;
};

export function SettingsDialog({
  projectRoot,
  onClose,
  onPrefsChange,
}: SettingsDialogProps) {
  const [section, setSection] = useState<SectionId>("general");
  const [prefs, setPrefs] = useState<GeneralPrefs | null>(null);
  const [paths, setPaths] = useState<SettingsPaths | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

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
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

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
                  <label
                    className="settings-field-label"
                    htmlFor="error-language"
                  >
                    Язык сообщений об ошибках
                  </label>
                  <select
                    id="error-language"
                    className="settings-select"
                    value={prefs?.errorLanguage ?? "ru"}
                    disabled={!prefs || busy}
                    onChange={(event) =>
                      patchPrefs({
                        errorLanguage: event.target.value as "ru" | "en",
                      })
                    }
                  >
                    <option value="ru">Русский</option>
                    <option value="en">English</option>
                  </select>
                  <p className="settings-hint">
                    Язык текста диагностик в панели «Проблемы» (битые include,
                    xref, изображения, циклы и т.п.). Применяется при следующей
                    переиндексации.
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
                <div className="settings-row">
                  <div className="settings-section-title">
                    Поддерживаемые форматы
                  </div>
                  <div className="settings-formats">
                    {SUPPORTED_FORMAT_LABELS.map((label) => (
                      <span key={label} className="settings-format-chip">
                        {label}
                      </span>
                    ))}
                  </div>
                </div>
              </>
            ) : null}

            {section === "paths" ? (
              <>
                <div className="settings-section-title">Пути</div>
                <div className="settings-row">
                  <span className="settings-hint" style={{ paddingLeft: 0 }}>
                    Папка настроек пользователя (~/.docflow)
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
                    Настройки проекта (.docflow)
                  </span>
                  <div
                    className={`settings-path${!paths?.projectDocflowDir && !projectRoot ? " empty" : ""}`}
                  >
                    {projectRoot
                      ? `${projectRoot.replace(/[/\\]+$/, "")}/.docflow`
                      : (paths?.projectDocflowDir ?? "—")}
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
