import { useCallback, useEffect, useState } from "react";
import { planDelete, planGet, planList, type PlanRecord, type PlanSummary } from "../../lib/plans";
import { PlanDetailView } from "./PlanDetailView";
import "../ToolLog/ToolCallLogModal.css";
import "./PlansModal.css";

type PlansModalProps = {
  initialPlanId?: string | null;
  onClose: () => void;
  onStartPlan?: (planId: string) => void;
  onOpenInEditor?: (planId: string) => void;
  startDisabled?: boolean;
};

function formatShortDate(ms: number): string {
  return new Date(ms).toLocaleString("ru-RU", {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function progressLabel(completed: number, total: number): string {
  if (total <= 0) return "без шагов";
  if (completed >= total) return "готово";
  if (completed === 0) return "не начат";
  return "в работе";
}

function progressKind(completed: number, total: number): "idle" | "active" | "done" {
  if (total <= 0 || completed === 0) return "idle";
  if (completed >= total) return "done";
  return "active";
}

export function PlansModal({
  initialPlanId,
  onClose,
  onStartPlan,
  onOpenInEditor,
  startDisabled,
}: PlansModalProps) {
  const [summaries, setSummaries] = useState<PlanSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(initialPlanId ?? null);
  const [detail, setDetail] = useState<PlanRecord | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const refreshList = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await planList();
      setSummaries(list);
      setSelectedId((prev) => {
        if (prev && list.some((p) => p.id === prev)) return prev;
        return list[0]?.id ?? null;
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSummaries([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshList();
  }, [refreshList]);

  useEffect(() => {
    if (initialPlanId) setSelectedId(initialPlanId);
  }, [initialPlanId]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  useEffect(() => {
    if (!selectedId) {
      setDetail(null);
      setConfirmingDelete(false);
      return;
    }
    let cancelled = false;
    setConfirmingDelete(false);
    void planGet(selectedId)
      .then((record) => {
        if (!cancelled) setDetail(record);
      })
      .catch((e: unknown) => {
        if (!cancelled) {
          setDetail(null);
          setError(e instanceof Error ? e.message : String(e));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  const handleDelete = async (planId: string) => {
    setDeleting(true);
    try {
      await planDelete(planId);
      setConfirmingDelete(false);
      await refreshList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setDeleting(false);
    }
  };

  const handleStart = (planId: string) => {
    onStartPlan?.(planId);
    onClose();
  };

  const rangeLabel =
    summaries.length === 0
      ? "0 планов"
      : `${summaries.length} ${summaries.length === 1 ? "план" : "планов"}`;

  return (
    <div className="tool-log-backdrop" role="presentation" onClick={onClose}>
      <div
        className="tool-log-dialog plans-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="plans-dialog-title"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="tool-log-header">
          <h2 className="tool-log-title" id="plans-dialog-title">
            Планы
          </h2>
          <div className="tool-log-header-actions">
            {confirmingDelete && detail ? (
              <>
                <span className="tool-log-confirm-text">Удалить «{detail.name}»?</span>
                <button
                  type="button"
                  className="tool-log-btn tool-log-btn-danger"
                  disabled={deleting}
                  onClick={() => void handleDelete(detail.id)}
                >
                  Да, удалить
                </button>
                <button
                  type="button"
                  className="tool-log-btn"
                  disabled={deleting}
                  onClick={() => setConfirmingDelete(false)}
                >
                  Отмена
                </button>
              </>
            ) : (
              <>
                {detail ? (
                  <button
                    type="button"
                    className="tool-log-btn"
                    onClick={() => setConfirmingDelete(true)}
                  >
                    Удалить
                  </button>
                ) : null}
                {detail && onOpenInEditor ? (
                  <button
                    type="button"
                    className="tool-log-btn"
                    onClick={() => onOpenInEditor(detail.id)}
                  >
                    Открыть во вкладке
                  </button>
                ) : null}
                {detail && onStartPlan ? (
                  <button
                    type="button"
                    className="tool-log-btn tool-log-btn-primary"
                    disabled={startDisabled}
                    onClick={() => handleStart(detail.id)}
                  >
                    Начать
                  </button>
                ) : null}
              </>
            )}
            <button type="button" className="tool-log-close" onClick={onClose} aria-label="Закрыть">
              ×
            </button>
          </div>
        </header>

        {error ? <div className="tool-log-error">{error}</div> : null}

        <div className="plans-layout">
          <aside className="plans-list-pane">
            <div className="plans-list-head">
              <span>Название</span>
              <span>Прогресс</span>
            </div>
            <div className="plans-list-body">
              {loading ? (
                <div className="tool-log-empty">Загрузка…</div>
              ) : summaries.length === 0 ? (
                <div className="tool-log-empty">Пока нет сохранённых планов</div>
              ) : (
                <ul className="plans-list">
                  {summaries.map((s) => {
                    const kind = progressKind(s.todoCompleted, s.todoTotal);
                    return (
                      <li key={s.id}>
                        <button
                          type="button"
                          className={`plans-list-item${selectedId === s.id ? " is-active" : ""}`}
                          onClick={() => setSelectedId(s.id)}
                        >
                          <span className="plans-list-main">
                            <span className="plans-list-name">{s.name}</span>
                            <span className="plans-list-meta">{formatShortDate(s.updatedAtMs)}</span>
                          </span>
                          <span className="plans-list-side">
                            <span className={`plans-progress plans-progress-${kind}`}>
                              {s.todoCompleted}/{s.todoTotal}
                            </span>
                            <span className="plans-list-status">{progressLabel(s.todoCompleted, s.todoTotal)}</span>
                          </span>
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          </aside>

          <section className="plans-detail-pane">
            {detail ? (
              <PlanDetailView plan={detail} />
            ) : (
              <div className="tool-log-empty">Выберите план слева</div>
            )}
          </section>
        </div>

        <footer className="tool-log-footer">
          <span className="tool-log-range">{loading ? "Загрузка…" : rangeLabel}</span>
        </footer>
      </div>
    </div>
  );
}
