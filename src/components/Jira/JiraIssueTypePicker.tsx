import { useEffect, useRef, useState } from "react";
import { jiraListIssueTypes, type JiraIssueType } from "../../lib/jira";
import { toMessage } from "../../lib/errors";
import "./JiraProjectPicker.css";

/** Picks the issue type new tickets are created with.
 *
 *  A trigger-and-menu dropdown, not a native `<select>`: the app renders its
 *  own (`.method-select*` in the HTTP builder, `.oas-select*` in the OpenAPI
 *  explorer, `.assistant-mode-*` in the chat), and a platform control looks
 *  foreign next to them — see the Style section of AGENTS.md.
 *
 *  A list rather than a search box, unlike the project picker: a project
 *  configures a couple of dozen types at most (21 in the busiest one here),
 *  which is something you read. Sub-task types never arrive — the backend
 *  drops them, since a sub-task needs a parent.
 *
 *  Types belong to the project, so the list is refetched whenever
 *  `projectKey` changes, and an empty project means there is nothing to
 *  choose from yet. */
export function JiraIssueTypePicker({
  projectKey,
  issueTypeId,
  issueTypeName,
  disabled,
  onPick,
}: {
  projectKey: string;
  issueTypeId: string;
  issueTypeName: string;
  disabled: boolean;
  onPick: (issueType: JiraIssueType) => void;
}) {
  const [types, setTypes] = useState<JiraIssueType[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!projectKey) {
      setTypes(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setTypes(null);
    jiraListIssueTypes(projectKey)
      .then((list) => {
        if (cancelled) return;
        setTypes(list);
        setError(null);
      })
      .catch((e) => {
        if (!cancelled) setError(toMessage(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [projectKey]);

  // Same dismissal contract as the app's other dropdowns: a click anywhere
  // outside, or Escape.
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  if (!projectKey) return null;

  const empty = types?.length === 0;
  const label = issueTypeName || issueTypeId;

  return (
    <div className="jira-picker">
      <span className="jira-picker-label">Тип задачи</span>

      <div className="jira-select" ref={rootRef}>
        <button
          type="button"
          className={`jira-select-trigger${open ? " is-open" : ""}`}
          aria-haspopup="listbox"
          aria-expanded={open}
          aria-label="Тип задачи"
          disabled={disabled || loading || empty}
          onClick={() => setOpen((value) => !value)}
        >
          <span className={`jira-select-value${label ? "" : " is-empty"}`}>
            {loading ? "Загрузка…" : label || "Не выбран"}
          </span>
          <span className="jira-select-chevron" aria-hidden>
            ▾
          </span>
        </button>

        {open ? (
          <div className="jira-select-menu" role="listbox">
            {(types ?? []).map((type) => (
              <button
                key={type.id}
                type="button"
                role="option"
                aria-selected={type.id === issueTypeId}
                className={`jira-select-option${type.id === issueTypeId ? " is-active" : ""}`}
                onClick={() => {
                  onPick(type);
                  setOpen(false);
                }}
              >
                {type.name}
              </button>
            ))}
          </div>
        ) : null}
      </div>

      {error ? <p className="jira-picker-error">{error}</p> : null}
      {empty ? (
        <p className="jira-picker-hint">В проекте нет типов, доступных для создания.</p>
      ) : null}
    </div>
  );
}
