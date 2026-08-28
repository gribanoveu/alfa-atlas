import { ShieldOff } from "lucide-react";
import { useToolPermissions } from "../../hooks/useToolPermissions";
import { AUTO_APPROVABLE_TOOL_LABELS } from "../../lib/assistantConfig";
import "./PermissionsTab.css";

/** Every `ToolName` variant (`src-tauri/src/domain/ai_access.rs`), in the
 * same order the Rust enum declares them — reuses `AUTO_APPROVABLE_TOOL_LABELS`
 * for the tools it already names rather than duplicating those labels. */
const ALLOWED_TOOL_LABELS: Record<string, string> = {
  listFiles: "Просмотр списка файлов (listFiles)",
  readFile: "Чтение файлов (readFile)",
  semanticSearch: "Семантический поиск (semanticSearch)",
  grep: "Точный поиск по содержимому (grep)",
  gitDiff: "Разница между состояниями файлов (gitDiff)",
  gitBlame: "Отслеживание изменений в файлах (gitBlame)",
  check: "Проверка документации (check)",
  getAsciidocTemplates: "Шаблоны элементов AsciiDoc (getAsciidocTemplates)",
  skill: "Скилы ассистента (skill)",
  askUser: "Уточняющие вопросы (askUser)",
  requestArtifact: "Запрос артефакта у пользователя (requestArtifact)",
  artifact: "Чтение артефактов (artifact)",
  ...AUTO_APPROVABLE_TOOL_LABELS,
  todo: "Список задач (todo)",
  createPlan: "Создание плана (createPlan)",
  updatePlan: "Обновление плана (updatePlan)",
  readPlan: "Чтение плана (readPlan)",
  updatePlanTodo: "Статус шага плана (updatePlanTodo)",
};

const ALLOWED_TOOL_ORDER = [
  "listFiles",
  "readFile",
  "semanticSearch",
  "grep",
  "gitDiff",
  "gitBlame",
  "check",
  "writeFile",
  "editFile",
  "deleteFile",
  "createDirectory",
  "deleteDirectory",
  "move",
  "requestFullRepoAccess",
  "todo",
  "requestModeSwitch",
  "getAsciidocTemplates",
  "skill",
  "askUser",
  "createPlan",
  "updatePlan",
  "readPlan",
  "updatePlanTodo",
];

/** Per-project "always allow" list for the assistant's tool-calling loop
 * (`ProjectConfig.ai_auto_approved_tools`, see AI_HARNESS.md's "Tool-calling
 * loop") — granting happens from an approval card's "Разрешать всегда"
 * button in the chat panel; this tab is the only place to revoke it. */
export function PermissionsTab() {
  const { autoApproved, allowed, revokeAutoApproval, toggleAllowed } = useToolPermissions();
  const { tools, loading, noProject, error, pending: revoking } = autoApproved;
  const {
    tools: allowedTools,
    loading: allowedLoading,
    noProject: allowedNoProject,
    error: allowedError,
    pending: togglingTool,
  } = allowed;

  return (
    <div className="settings-sections permissions-tab">
      <div className="settings-card">
        <div className="settings-section-title">Разрешённые инструменты</div>
        <p className="settings-hint settings-hint-compact">
          Что ассистент вообще может предлагать — независимо от того, требуют ли эти
          действия подтверждения. Отключённый инструмент модель не сможет вызвать вовсе.
        </p>

        {allowedLoading ? (
          <p className="settings-hint settings-hint-compact">
            Загрузка…
          </p>
        ) : allowedNoProject ? (
          <p className="settings-hint settings-hint-compact">
            Откройте проект, чтобы посмотреть и изменить список разрешённых инструментов.
          </p>
        ) : (
          <ul className="permissions-list">
            {ALLOWED_TOOL_ORDER.map((tool) => (
              <li key={tool} className="permissions-item">
                <label className="permissions-item-label permissions-item-checkbox-label">
                  <input
                    type="checkbox"
                    checked={allowedTools.includes(tool)}
                    disabled={togglingTool === tool}
                    onChange={(e) => void toggleAllowed(tool, e.target.checked)}
                  />
                  {ALLOWED_TOOL_LABELS[tool] ?? tool}
                </label>
              </li>
            ))}
          </ul>
        )}

        {allowedError ? <div className="settings-error">{allowedError}</div> : null}
      </div>

      <div className="settings-card">
        <div className="settings-section-title">Автоматически одобренные действия</div>
        <p className="settings-hint settings-hint-compact">
          Разрешены ранее кнопкой «Разрешать всегда» на карточке запроса и теперь
          выполняются сразу. Отзовите здесь то, что больше не должно выполняться
          автоматически.
        </p>

        {loading ? (
          <p className="settings-hint settings-hint-compact">
            Загрузка…
          </p>
        ) : noProject ? (
          <p className="settings-hint settings-hint-compact">
            Откройте проект, чтобы посмотреть и изменить его список автоматически одобренных
            действий.
          </p>
        ) : tools.length === 0 ? (
          <p className="settings-hint settings-hint-compact">
            Для этого проекта ничего не одобрено автоматически — каждое изменяющее действие
            по-прежнему требует подтверждения.
          </p>
        ) : (
          <ul className="permissions-list">
            {tools.map((tool) => (
              <li key={tool} className="permissions-item">
                <span className="permissions-item-label">{AUTO_APPROVABLE_TOOL_LABELS[tool] ?? tool}</span>
                <button
                  type="button"
                  className="settings-link-btn danger permissions-item-revoke"
                  disabled={revoking === tool}
                  onClick={() => void revokeAutoApproval(tool)}
                >
                  <ShieldOff size={14} aria-hidden />
                  {revoking === tool ? "Отзывается…" : "Отозвать"}
                </button>
              </li>
            ))}
          </ul>
        )}

        {error ? <div className="settings-error">{error}</div> : null}
      </div>
    </div>
  );
}
