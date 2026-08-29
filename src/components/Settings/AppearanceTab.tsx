import { Fragment } from "react";
import type { GeneralPrefsEditor } from "../../hooks/useGeneralPrefsEditor";
import {
  clampFontSizePx,
  DEFAULT_DIAGRAM_BACKDROP,
  DEFAULT_GENERAL_PREFS,
  DIAGRAM_BACKDROP_PRESETS,
  FONT_SIZE_LIMITS,
  normalizeDiagramBackdrop,
  type GeneralPrefs,
} from "../../lib/prefs";

type FontSizePrefKey =
  | "uiFontSizePx"
  | "sidebarFontSizePx"
  | "editorFontSizePx"
  | "previewFontSizePx"
  | "assistantFontSizePx";

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
  {
    key: "assistantFontSizePx",
    id: "font-assistant-size",
    label: "Ассистент",
    hint: "Текст ответов в чате",
  },
];

type AppearanceTabProps = {
  editor: GeneralPrefsEditor;
};

/** Everything that only changes how the app looks: per-zone font sizes and
 * how the file tree lays itself out. */
export function AppearanceTab({ editor }: AppearanceTabProps) {
  const { prefs, error, busy, patchPrefs, stagePref, persistPref, resetFontPrefs } = editor;
  const backdrop = normalizeDiagramBackdrop(
    prefs?.diagramBackdrop ?? DEFAULT_GENERAL_PREFS.diagramBackdrop,
  );

  return (
    <div className="settings-sections">
      <div className="settings-card">
        <div className="settings-section-head">
          <div className="settings-section-title">Размер шрифта</div>
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
                  stagePref({ [key]: clampFontSizePx(raw) } as Pick<
                    GeneralPrefs,
                    FontSizePrefKey
                  >);
                }}
                onBlur={() => {
                  if (!prefs) return;
                  void persistPref({ [key]: clampFontSizePx(prefs[key]) } as Pick<
                    GeneralPrefs,
                    FontSizePrefKey
                  >);
                }}
              />
            </Fragment>
          ))}
        </div>
        <p className="settings-hint settings-hint-compact">
          {FONT_SIZE_LIMITS.min}–{FONT_SIZE_LIMITS.max} px, шаг{" "}
          {FONT_SIZE_LIMITS.step}. Изменения видны сразу, сохранение — при потере
          фокуса.
        </p>
      </div>

      <div className="settings-card">
        <div className="settings-section-title">Дерево файлов</div>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={prefs?.separateExternalFolder ?? true}
            disabled={!prefs || busy}
            onChange={(event) =>
              patchPrefs({ separateExternalFolder: event.target.checked })
            }
          />
          <span>Отдельно показывать папку _external</span>
        </label>
        <p className="settings-hint">
          Если в корне документации есть папка _external, она отображается
          отдельным блоком под основным деревом.
        </p>
      </div>

      <div className="settings-card">
        <div className="settings-section-head">
          <div className="settings-section-title">Подложка диаграмм</div>
          <button
            type="button"
            className="settings-link-btn"
            disabled={!prefs || busy || backdrop === DEFAULT_DIAGRAM_BACKDROP}
            onClick={() => patchPrefs({ diagramBackdrop: DEFAULT_DIAGRAM_BACKDROP })}
          >
            Сбросить
          </button>
        </div>
        <div className="settings-backdrop-row">
          {DIAGRAM_BACKDROP_PRESETS.map((preset) => (
            <button
              key={preset.value}
              type="button"
              className={`settings-backdrop-preset${
                backdrop === preset.value ? " is-active" : ""
              }${preset.value === "transparent" ? " is-transparent" : ""}`}
              style={
                preset.value === "transparent" ? undefined : { background: preset.value }
              }
              disabled={!prefs || busy}
              aria-pressed={backdrop === preset.value}
              title={preset.label}
              onClick={() => patchPrefs({ diagramBackdrop: preset.value })}
            >
              <span className="settings-backdrop-preset-label">{preset.label}</span>
            </button>
          ))}
          <label className="settings-backdrop-custom">
            <input
              type="color"
              className="settings-backdrop-input"
              // `<input type="color">` has no notion of `transparent`, so it
              // shows the default swatch while that preset is picked rather
              // than pretending some colour is selected.
              value={backdrop === "transparent" ? DEFAULT_DIAGRAM_BACKDROP : backdrop}
              disabled={!prefs || busy}
              aria-label="Свой цвет подложки"
              onChange={(event) =>
                patchPrefs({ diagramBackdrop: normalizeDiagramBackdrop(event.target.value) })
              }
            />
            <span>Свой цвет</span>
          </label>
        </div>
        <p className="settings-hint settings-hint-compact">
          Фон под отрисованными схемами Mermaid и PlantUML — в превью документов
          и во вкладках схем от ассистента. Обе библиотеки рисуют тёмным по
          светлому, поэтому на тёмной или прозрачной подложке диаграмма может
          читаться хуже.
        </p>
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
