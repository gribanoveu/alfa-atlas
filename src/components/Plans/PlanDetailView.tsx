import { AssistantMarkdown } from "../RightDock/AssistantMarkdown";
import type { PlanRecord, PlanTodoStatus } from "../../lib/plans";

function todoLabel(status: PlanTodoStatus): string {
  switch (status) {
    case "completed":
      return "готово";
    case "inProgress":
      return "сейчас";
    case "cancelled":
      return "отменено";
    case "pending":
      return "ожидает";
  }
}

function formatTs(ms: number): string {
  return new Date(ms).toLocaleString("ru-RU", {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function PlanDetailView({ plan }: { plan: PlanRecord }) {
  const completed = plan.todos.filter((t) => t.status === "completed").length;
  const total = plan.todos.filter((t) => t.status !== "cancelled").length;
  const pct = total > 0 ? Math.round((completed / total) * 100) : 0;

  return (
    <div className="plan-detail">
      <header className="plan-detail-header">
        <h3 className="plan-detail-title">{plan.name}</h3>
        {plan.overview ? <p className="plan-detail-overview">{plan.overview}</p> : null}
        <div className="plan-detail-meta-row">
          <span className="plan-detail-meta">
            Обновлён {formatTs(plan.updatedAtMs)}
            {plan.createdAtMs !== plan.updatedAtMs
              ? ` · создан ${formatTs(plan.createdAtMs)}`
              : null}
          </span>
          <span className="plan-detail-progress-label">
            {completed}/{total} шагов · {pct}%
          </span>
        </div>
        <div className="plan-detail-progress-track" aria-hidden>
          <div className="plan-detail-progress-fill" style={{ width: `${pct}%` }} />
        </div>
      </header>

      <div className="plan-detail-scroll">
        <section className="plan-detail-section">
          <div className="plan-detail-section-label">План</div>
          <div className="plan-detail-markdown">
            <AssistantMarkdown content={plan.plan} streaming={false} />
          </div>
        </section>

        <section className="plan-detail-section">
          <div className="plan-detail-section-label">Шаги</div>
          {plan.todos.length === 0 ? (
            <div className="plan-detail-todos-empty">Шагов пока нет</div>
          ) : (
            <ol className="plan-detail-todos">
              {plan.todos.map((t, i) => (
                <li key={t.id} className={`plan-detail-todo is-${t.status}`}>
                  <span className="plan-detail-todo-index" aria-hidden>
                    {i + 1}
                  </span>
                  <div className="plan-detail-todo-body">
                    <span className="plan-detail-todo-content">{t.content}</span>
                    {t.note ? <span className="plan-detail-todo-note">{t.note}</span> : null}
                  </div>
                  <span className={`plan-detail-todo-status is-${t.status}`}>
                    {todoLabel(t.status)}
                  </span>
                </li>
              ))}
            </ol>
          )}
        </section>
      </div>
    </div>
  );
}
