import { memo, useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronUp, ListTodo } from "lucide-react";
import { planGet, type PlanTodo, type PlanTodoStatus } from "../../lib/plans";

function todoGlyph(status: PlanTodoStatus): string {
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

const TodoRow = memo(
  function TodoRow({ task, index }: { task: PlanTodo; index: number }) {
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
        <span className="assistant-todo-row-title">{task.content}</span>
      </li>
    );
  },
  (prev, next) =>
    prev.index === next.index &&
    prev.task.id === next.task.id &&
    prev.task.content === next.task.content &&
    prev.task.status === next.task.status &&
    prev.task.note === next.task.note,
);

const AUTO_COLLAPSE_DELAY_MS = 2000;

/** Progress card for the active persisted work plan — same layout as
 * `TodoProgressWidget`, but backed by `planGet(activePlanId)`. */
export function PlanProgressWidget({
  planId,
  refreshKey,
}: {
  planId: string;
  /** Bump when a tool result may have changed todos (e.g. message count). */
  refreshKey?: number | string;
}) {
  const [tasks, setTasks] = useState<PlanTodo[]>([]);
  const [planName, setPlanName] = useState("План");
  const [expanded, setExpanded] = useState(false);
  const wasDoneRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    void planGet(planId)
      .then((record) => {
        if (cancelled) return;
        setTasks(record.todos);
        setPlanName(record.name);
      })
      .catch(() => {
        if (!cancelled) setTasks([]);
      });
    return () => {
      cancelled = true;
    };
  }, [planId, refreshKey]);

  const current = tasks.find((t) => t.status === "inProgress");
  const completed = tasks.filter((t) => t.status === "completed").length;
  const cancelledCount = tasks.filter((t) => t.status === "cancelled").length;
  const total = tasks.length - cancelledCount;
  const remaining = tasks.filter((t) => t.status === "pending" || t.status === "inProgress").length;
  const isDone = tasks.length > 0 && remaining === 0;

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

  const currentPosition = completed + 1;
  const summaryText = isDone
    ? `План «${planName}» выполнен`
    : current
      ? `План «${planName}»: шаг ${currentPosition} из ${total}`
      : `План «${planName}»`;

  return (
    <div className={`assistant-todo-widget${isDone ? " is-done" : ""}`}>
      <div className="assistant-todo-widget-header-row">
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
      </div>
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
