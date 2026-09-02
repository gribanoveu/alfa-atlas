import { useEffect, useState } from "react";
import { jiraListProjects, type JiraProject } from "../../lib/jira";
import { toMessage } from "../../lib/errors";
import "./JiraProjectPicker.css";

/** How many matches to show while searching. The instance has thousands of
 *  projects; a list longer than this is a sign the query is too broad, not
 *  something to scroll. */
const SEARCH_LIMIT = 40;

/** Picks the project new issues go to, and remembers it.
 *
 *  Opens on the projects the user last worked in (`recent`) rather than on
 *  the full list: this instance answers with ~2300 projects, which is a
 *  scroll bar, not a choice. Typing switches to a search over all of them —
 *  fetched once and filtered here, so a keystroke is not a request. */
export function JiraProjectPicker({
  projectKey,
  projectName,
  disabled,
  onPick,
}: {
  projectKey: string;
  projectName: string;
  disabled: boolean;
  onPick: (project: JiraProject) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [recent, setRecent] = useState<JiraProject[] | null>(null);
  const [all, setAll] = useState<JiraProject[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const trimmed = query.trim().toLowerCase();

  useEffect(() => {
    if (!open || recent) return;
    setLoading(true);
    jiraListProjects(true)
      .then((list) => {
        setRecent(list);
        setError(null);
      })
      .catch((e) => setError(toMessage(e)))
      .finally(() => setLoading(false));
  }, [open, recent]);

  // The full list is fetched once, on the first search — never on open,
  // because most of the time the recent ones are the answer.
  useEffect(() => {
    if (!open || !trimmed || all) return;
    setLoading(true);
    jiraListProjects(false)
      .then((list) => {
        setAll(list);
        setError(null);
      })
      .catch((e) => setError(toMessage(e)))
      .finally(() => setLoading(false));
  }, [open, trimmed, all]);

  const shown = trimmed
    ? (all ?? [])
        .filter(
          (p) =>
            p.key.toLowerCase().includes(trimmed) || p.name.toLowerCase().includes(trimmed),
        )
        // A key match is what someone typing «WOW» means; a name match is a
        // fallback, so it sorts after.
        .sort((a, b) => {
          const aKey = a.key.toLowerCase().startsWith(trimmed) ? 0 : 1;
          const bKey = b.key.toLowerCase().startsWith(trimmed) ? 0 : 1;
          return aKey - bKey || a.key.localeCompare(b.key);
        })
        .slice(0, SEARCH_LIMIT)
    : (recent ?? []);

  return (
    <div className="jira-picker">
      <span className="jira-picker-label">Проект для новых задач</span>

      <div className="jira-project-row">
        <span className="jira-project-current">
          {projectKey ? (
            <>
              <span className="jira-project-key">{projectKey}</span>
              {projectName ? (
                <span className="jira-project-name">{projectName}</span>
              ) : null}
            </>
          ) : (
            <span className="jira-project-empty">Не выбран</span>
          )}
        </span>
        <button
          type="button"
          className="jira-picker-btn"
          disabled={disabled}
          onClick={() => setOpen((value) => !value)}
        >
          {open ? "Скрыть" : projectKey ? "Изменить" : "Выбрать"}
        </button>
      </div>

      {open ? (
        <div className="jira-project-picker">
          <input
            className="jira-picker-input"
            type="text"
            placeholder="Поиск по ключу или названию"
            aria-label="Поиск проекта"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
          {error ? <p className="jira-picker-error">{error}</p> : null}
          {loading ? <p className="jira-picker-hint">Загрузка…</p> : null}
          {!loading && shown.length === 0 ? (
            <p className="jira-picker-hint">
              {trimmed ? "Ничего не найдено" : "Список пуст"}
            </p>
          ) : null}
          <ul className="jira-project-list" role="list">
            {shown.map((project) => (
              <li key={project.key}>
                <button
                  type="button"
                  className={`jira-project-option${project.key === projectKey ? " is-active" : ""}`}
                  onClick={() => {
                    onPick(project);
                    setOpen(false);
                    setQuery("");
                  }}
                >
                  <span className="jira-project-key">{project.key}</span>
                  <span className="jira-project-name">{project.name}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      ) : null}

      <p className="jira-picker-hint">
        {trimmed
          ? "Поиск идёт по всем проектам инстанса."
          : "Показаны проекты, в которых вы работали недавно. Начните печатать, чтобы искать по всем."}
      </p>
    </div>
  );
}
