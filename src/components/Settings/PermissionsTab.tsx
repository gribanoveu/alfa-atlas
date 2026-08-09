import { ShieldOff } from "lucide-react";
import { useEffect, useState } from "react";
import { getAutoApprovedTools, setToolAutoApproved } from "../../lib/aiTools";
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

  return (
    <div className="permissions-tab">
      <div className="settings-section-title">Автоматически одобренные действия</div>
      <p className="settings-lead">
        Для текущего открытого проекта. Когда ассистент запрашивает изменение файлов, эти
        действия выполняются сразу, без подтверждения — вы разрешили это ранее кнопкой
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
