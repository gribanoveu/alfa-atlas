import type { GeneralPrefsEditor } from "../../hooks/useGeneralPrefsEditor";
import { AUTOSAVE_DELAY_LIMITS, clampAutosaveDelayMs } from "../../lib/prefs";

type EditorTabProps = {
  editor: GeneralPrefsEditor;
};

/** How editing behaves: when a buffer reaches disk, and how the editor
 * resolves OpenAPI `$ref`s it cannot find. */
export function EditorTab({ editor }: EditorTabProps) {
  const { prefs, error, busy, patchPrefs, stagePref, persistPref } = editor;
  const autosaveOn = prefs?.autosaveEnabled ?? true;

  return (
    <div className="settings-sections">
      <div className="settings-card">
        <div className="settings-section-title">Автосохранение</div>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={autosaveOn}
            disabled={!prefs || busy}
            onChange={(event) => patchPrefs({ autosaveEnabled: event.target.checked })}
          />
          <span>Автосохранение при редактировании</span>
        </label>
        <p className="settings-hint">
          Периодически записывает файл на диск после паузы ввода.
        </p>

        <hr className="settings-card-divider" />

        <label className="settings-check">
          <input
            type="checkbox"
            checked={prefs?.saveOnTabSwitch ?? true}
            disabled={!prefs || busy}
            onChange={(event) => patchPrefs({ saveOnTabSwitch: event.target.checked })}
          />
          <span>Сохранять при переключении вкладки</span>
        </label>
        <p className="settings-hint">
          Незаписанные изменения уходят на диск, как только вы уходите с вкладки.
        </p>

        <hr className="settings-card-divider" />

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
          disabled={!prefs || busy || !autosaveOn}
          onChange={(event) => {
            if (!prefs) return;
            const raw = Number(event.target.value);
            if (!Number.isFinite(raw)) return;
            stagePref({ autosaveDelayMs: clampAutosaveDelayMs(raw) });
          }}
          onBlur={() => {
            if (!prefs) return;
            persistPref({
              autosaveDelayMs: clampAutosaveDelayMs(prefs.autosaveDelayMs),
            });
          }}
        />
        <p className="settings-hint settings-hint-compact">
          От {AUTOSAVE_DELAY_LIMITS.min} до {AUTOSAVE_DELAY_LIMITS.max} мс.
        </p>
      </div>

      <div className="settings-card">
        <div className="settings-section-title">OpenAPI</div>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={prefs?.openApiRefFallbackEnabled ?? true}
            disabled={!prefs || busy}
            onChange={(event) =>
              patchPrefs({ openApiRefFallbackEnabled: event.target.checked })
            }
          />
          <span>Подставлять встроенный common-спек OpenAPI, если файл не найден</span>
        </label>
        <p className="settings-hint">
          Если $ref ссылается на build/common/META-INF/specs/api.yaml
          (build-артефакт Java/Gradle-проекта, появляющийся только после сборки)
          и этого файла нет на диске, редактор подставляет встроенную копию этого
          common-спека вместо диагностики «file not found».
        </p>
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
