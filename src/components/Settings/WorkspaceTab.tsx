import type { GeneralPrefsEditor } from "../../hooks/useGeneralPrefsEditor";
import { SUPPORTED_FORMAT_LABELS } from "../../lib/supportedFiles";

type WorkspaceTabProps = {
  projectRoot: string | null;
  editor: GeneralPrefsEditor;
};

/** Read-only reference: where Alfa Atlas keeps its files, and which formats
 * it can open. The one action here is revealing the settings folder. */
export function WorkspaceTab({ projectRoot, editor }: WorkspaceTabProps) {
  const { paths, error, openUserSettingsDir } = editor;

  const projectConfigDir = projectRoot
    ? `${projectRoot.replace(/[/\\]+$/, "")}/.atlas`
    : (paths?.projectConfigDir ?? null);

  const rows: { label: string; value: string | null; placeholder: string }[] = [
    {
      label: "Настройки пользователя",
      value: paths?.userSettingsDir ?? null,
      placeholder: "…",
    },
    { label: "Планы", value: paths?.plansDir ?? null, placeholder: "…" },
    { label: "Артефакты", value: paths?.artifactsDir ?? null, placeholder: "…" },
    {
      label: "Текущий проект",
      value: projectRoot ?? paths?.projectRoot ?? null,
      placeholder: "Проект не открыт",
    },
    {
      label: "Настройки проекта",
      value: projectConfigDir,
      placeholder: "Проект не открыт",
    },
  ];

  return (
    <div className="settings-sections">
      <div className="settings-card">
        <div className="settings-section-title">Расположение файлов</div>
        <dl className="settings-path-list">
          {rows.map((row) => (
            <div key={row.label} className="settings-path-row">
              <dt className="settings-path-label">{row.label}</dt>
              <dd className={`settings-path${row.value ? "" : " empty"}`}>
                {row.value ?? row.placeholder}
              </dd>
            </div>
          ))}
        </dl>
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
      </div>

      <div className="settings-card">
        <div className="settings-section-title">Поддерживаемые форматы</div>
        <p className="settings-hint settings-hint-compact">
          Файлы с этими расширениями открываются и редактируются в приложении.
        </p>
        <div className="settings-formats">
          {SUPPORTED_FORMAT_LABELS.map((label) => (
            <span key={label} className="settings-format-chip">
              {label}
            </span>
          ))}
        </div>
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
