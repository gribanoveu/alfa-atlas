import { useCallback, useEffect, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import { planDelete, planGet, planList, type PlanRecord, type PlanSummary } from "../lib/plans";

/** The saved plan list and whichever plan is currently selected.
 *
 * Selection survives a refresh when it still exists, and otherwise falls to
 * the first plan — so deleting the selected one lands somewhere sensible
 * instead of on an empty pane. */
export function usePlans(initialPlanId: string | null) {
  const [summaries, setSummaries] = useState<PlanSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(initialPlanId ?? null);
  const [detail, setDetail] = useState<PlanRecord | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [deleting, setDeleting] = useState(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const refreshList = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await planList();
      if (!mounted.current) return;
      setSummaries(list);
      setSelectedId((prev) => (prev && list.some((p) => p.id === prev) ? prev : list[0]?.id ?? null));
    } catch (e) {
      if (!mounted.current) return;
      setError(toMessage(e));
      setSummaries([]);
    } finally {
      if (mounted.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshList();
  }, [refreshList]);

  useEffect(() => {
    if (initialPlanId) setSelectedId(initialPlanId);
  }, [initialPlanId]);

  useEffect(() => {
    if (!selectedId) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    void planGet(selectedId)
      .then((record) => {
        if (!cancelled) setDetail(record);
      })
      .catch((e: unknown) => {
        if (cancelled) return;
        setDetail(null);
        setError(toMessage(e));
      });
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  /** `true` when the plan was actually removed, so the caller can close its
   * confirmation only on success. */
  const deletePlan = useCallback(
    async (planId: string) => {
      setDeleting(true);
      try {
        await planDelete(planId);
        await refreshList();
        return true;
      } catch (e) {
        if (mounted.current) setError(toMessage(e));
        return false;
      } finally {
        if (mounted.current) setDeleting(false);
      }
    },
    [refreshList],
  );

  return {
    summaries,
    selectedId,
    setSelectedId,
    detail,
    error,
    loading,
    deleting,
    refreshList,
    deletePlan,
  };
}
