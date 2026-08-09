import { memo, useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronUp, ListTodo } from "lucide-react";
import type { Task, TodoStatus } from "../../lib/aiTools";

function todoGlyph(status: TodoStatus): string {
  switch (status) {
    case "completed":
      return "✓";
    case "inProgress":
      return "●";
    case "cancelled":
      return "✗";
    case "pending":
      return "○";
  }
}

/** One row in the expanded checklist. Wrapped in `memo` with a field-level
 * comparator (not the default reference check) because every `Task` the
 * backend sends is a freshly deserialized object — the whole array is new
 * on every `todo` call, so reference equality would never skip a re-render.
 * Comparing by value means an untouched task (same id/title/status/note)
 * still skips its own DOM diff even though its containing array changed —
 * the "update only the row that actually changed" the spec asks for, given
 * React's rendering model doesn't otherwise offer a cheaper primitive than
 * this. `index` is the row's fixed 1-based position — safe to compare
 * alongside the rest since the list is append-only (an existing task's
 * position never shifts once assigned). */
const TodoRow = memo(
  function TodoRow({ task, index }: { task: Task; index: number }) {
    const cancelled = task.status === "cancelled";
    return (
      <li
        className={`assistant-todo-row${cancelled ? " is-cancelled" : ""}`}
        title={cancelled && task.note ? task.note : undefined}
      >
        <span className="assistant-todo-row-index">{index}.</span>
        <span className="assistant-todo-row-glyph" aria-hidden>
          {todoGlyph(task.status)}
        </span>
        <span className="assistant-todo-row-title">{task.title}</span>
      </li>
    );
  },
  (prev, next) =>
    prev.index === next.index &&
    prev.task.id === next.task.id &&
    prev.task.title === next.task.title &&
    prev.task.status === next.task.status &&
    prev.task.note === next.task.note,
);

const AUTO_COLLAPSE_DELAY_MS = 2000;

/** Floating progress card for the assistant's `todo` checklist — rendered
 * as a plain sibling right above `.assistant-chat-messages` (a flex-column
 * layout, see `AssistantPanel.css`), which is enough to keep it visibly
 * pinned above the scrolling transcript without `position: sticky`. Styled
 * with margin/rounded corners/shadow (not flush with the panel chrome) so
 * it reads as a card floating over the chat, not another strip of the
 * panel itself.
 *
 * Subscribes to `useLlmChat`'s `todos` state directly (not the chat
 * transcript) — the same value threaded through every `streamLlmChat`
 * call, so this can never drift from what the model itself sees, per the
 * "one source of truth" requirement. Purely a view: nothing here can
 * change a task's status — only the agent does that via `todo update`. */
export function TodoProgressWidget({ tasks }: { tasks: Task[] }) {
  const [expanded, setExpanded] = useState(false);
  const wasDoneRef = useRef(false);

  const current = tasks.find((t) => t.status === "inProgress");
  const completed = tasks.filter((t) => t.status === "completed").length;
  const cancelled = tasks.filter((t) => t.status === "cancelled").length;
  const total = tasks.length - cancelled;
  const remaining = tasks.filter((t) => t.status === "pending" || t.status === "inProgress").length;
  const isDone = tasks.length > 0 && remaining === 0;

  // Two symmetric transitions, both driven off `wasDoneRef` (not `tasks`
  // identity, so a manual re-expand of a finished checklist never
  // retriggers either branch):
  // - Just finished: collapse back to the compact "Готово" line after a
  //   couple of seconds, but only if the widget happened to be expanded —
  //   an already-collapsed widget has nothing to animate.
  // - Resumed after having been finished (the agent appended more work via
  //   another `write` once the previous plan was fully done — the list is
  //   append-only, there's no separate "plan 2"): force it open instead of
  //   leaving a stale "Готово" line collapsed while the count underneath it
  //   quietly changes. Without this, a user who isn't watching closely
  //   could easily miss that the plan grew new steps.
  useEffect(() => {
    if (isDone && !wasDoneRef.current) {
      wasDoneRef.current = true;
      if (expanded) {
        const timer = setTimeout(() => setExpanded(false), AUTO_COLLAPSE_DELAY_MS);
        return () => clearTimeout(timer);
      }
      return;
    }
    if (!isDone && wasDoneRef.current) {
      wasDoneRef.current = false;
      setExpanded(true);
    }
  }, [isDone, expanded]);

  if (tasks.length === 0) return null;

  // The current task's ordinal position among the counted (non-cancelled)
  // tasks — completed items plus this one, e.g. 2 done + this one = "3 из
  // 6". Deliberately not the raw array index: a cancelled task earlier in
  // the list shouldn't shift the number the user reads as "which step am I
  // on."
  const currentPosition = completed + 1;
  const summaryText = isDone
    ? "План выполнен"
    : current
      ? `В работе пункт плана ${currentPosition} из ${total}`
      : "Ожидание";

  return (
    <div className={`assistant-todo-widget${isDone ? " is-done" : ""}`}>
      <button
        type="button"
        className="assistant-todo-widget-header"
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
      >
        <ListTodo className="assistant-todo-widget-icon" size={14} aria-hidden />
        <span className="assistant-todo-widget-summary">{summaryText}</span>
        {expanded ? (
          <ChevronUp className="assistant-todo-widget-chevron" size={13} aria-hidden />
        ) : (
          <ChevronDown className="assistant-todo-widget-chevron" size={13} aria-hidden />
        )}
      </button>
      {expanded ? (
        <ul className="assistant-todo-widget-list">
          {tasks.map((t, i) => (
            <TodoRow key={t.id} task={t} index={i + 1} />
          ))}
        </ul>
      ) : null}
    </div>
  );
}
