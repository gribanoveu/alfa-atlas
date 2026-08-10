import { ShieldOff } from "lucide-react";
import { useEffect, useState } from "react";
import { getAllowedTools, getAutoApprovedTools, setToolAllowed, setToolAutoApproved } from "../../lib/aiTools";
import "./PermissionsTab.css";

/** Static Russian labels for the tools an approval card's "Разрешать
 * всегда" button can apply to (`ToolName::requires_confirmation` in Rust —
 * `Todo` is never among them, see `AI_HARNESS.md`'s "Tool-calling loop").
 * Falls back to the raw wire name for anything unrecognized, so a future
 * tool never silently disappears from this list before this map is
 * updated. */
const AUTO_APPROVABLE_TOOL_LABELS: Record<string, string> = {
  writeFile: "Запись файлов (writeFile)",
  editFile: "Редактирование файлов (editFile)",
  deleteFile: "Удаление файлов (deleteFile)",
  createDirectory: "Создание папок (createDirectory)",
  deleteDirectory: "Удаление папок (deleteDirectory)",
  move: "Перемещение / переименование (move)",
  requestFullRepoAccess: "Запрос доступа к репозиторию (requestFullRepoAccess)",
};

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
  ...AUTO_APPROVABLE_TOOL_LABELS,
  todo: "Список задач (todo)",
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
];

/** Per-project "always allow" list for the assistant's tool-calling loop
 * (`ProjectConfig.ai_auto_approved_tools`, see AI_HARNESS.md's "Tool-calling
 * loop") — granting happens from an approval card's "Разрешать всегда"
 * button in the chat panel; this tab is the only place to revoke it. */
export function PermissionsTab() {
  // Unlike every other Settings tab, this one is scoped to the currently
  // open project rather than global `AppSettings` — so `getAutoApprovedTools`
  // can fail with "no project is open" on a perfectly healthy install, which
  // is not an error worth an `.settings-error` banner; `noProject` renders a
  // plain hint instead.
  const [tools, setTools] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [noProject, setNoProject] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);

  // Independent state for the "which tools are allowed at all" section
  // below — a separate backend call (`ai_get_allowed_tools`/
  // `ai_set_tool_allowed`), same "no project is open" degrade shape.
  const [allowedTools, setAllowedTools] = useState<string[]>([]);
  const [allowedLoading, setAllowedLoading] = useState(true);
  const [allowedNoProject, setAllowedNoProject] = useState(false);
  const [allowedError, setAllowedError] = useState<string | null>(null);
  const [togglingTool, setTogglingTool] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const next = await getAutoApprovedTools();
        if (!cancelled) {
          setTools(next);
          setNoProject(false);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) {
          const message = e instanceof Error ? e.message : String(e);
          if (message.includes("no project is open")) {
            setNoProject(true);
          } else {
            setError(message);
          }
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const next = await getAllowedTools();
        if (!cancelled) {
          setAllowedTools(next);
          setAllowedNoProject(false);
          setAllowedError(null);
        }
      } catch (e) {
        if (!cancelled) {
          const message = e instanceof Error ? e.message : String(e);
          if (message.includes("no project is open")) {
            setAllowedNoProject(true);
          } else {
            setAllowedError(message);
          }
        }
      } finally {
        if (!cancelled) setAllowedLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleRevoke = async (tool: string) => {
    setRevoking(tool);
    try {
      await setToolAutoApproved(tool, false);
      setTools((prev) => prev.filter((t) => t !== tool));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRevoking(null);
    }
  };

  const handleToggleAllowed = async (tool: string, allowed: boolean) => {
    setTogglingTool(tool);
    try {
      await setToolAllowed(tool, allowed);
      setAllowedTools((prev) => (allowed ? [...prev, tool] : prev.filter((t) => t !== tool)));
    } catch (e) {
      setAllowedError(e instanceof Error ? e.message : String(e));
    } finally {
      setTogglingTool(null);
    }
  };

  return (
    <div className="permissions-tab">
      <div className="settings-section-title">Разрешённые инструменты</div>
      <p className="settings-lead">
        Для текущего открытого проекта. Какие действия ассистент вообще может предлагать —
        независимо от того, требуют ли они подтверждения. Отключённый инструмент модель не
        сможет вызвать вовсе.
      </p>

      {allowedLoading ? (
        <p className="settings-hint" style={{ paddingLeft: 0 }}>
          Загрузка…
        </p>
      ) : allowedNoProject ? (
        <p className="settings-hint" style={{ paddingLeft: 0 }}>
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
                  onChange={(e) => void handleToggleAllowed(tool, e.target.checked)}
                />
                {ALLOWED_TOOL_LABELS[tool] ?? tool}
              </label>
            </li>
          ))}
        </ul>
      )}

      {allowedError ? <div className="settings-error">{allowedError}</div> : null}

      <hr className="permissions-divider" />

      <div className="settings-section-title">Автоматически одобренные действия</div>
      <p className="settings-lead">
        Для текущего открытого проекта. Когда ассистент выполняет эти
        действия, они выполняются сразу. Они разрешены ранее кнопкой
        «Разрешать всегда» на карточке запроса. Отзовите здесь то, что больше не должно
        выполняться автоматически.
      </p>

      {loading ? (
        <p className="settings-hint" style={{ paddingLeft: 0 }}>
          Загрузка…
        </p>
      ) : noProject ? (
        <p className="settings-hint" style={{ paddingLeft: 0 }}>
          Откройте проект, чтобы посмотреть и изменить его список автоматически одобренных
          действий.
        </p>
      ) : tools.length === 0 ? (
        <p className="settings-hint" style={{ paddingLeft: 0 }}>
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
                onClick={() => void handleRevoke(tool)}
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
  );
}
