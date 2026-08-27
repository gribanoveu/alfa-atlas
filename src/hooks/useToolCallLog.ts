import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import { clearToolCallLog, queryToolCallLog, type ToolCallLogRow } from "../lib/toolCallLog";

export const TOOL_CALL_LOG_PAGE_SIZE = 50;

export type DateRangeOption = "all" | "hour" | "day" | "week";

export const DATE_RANGE_OPTIONS: { value: DateRangeOption; label: string }[] = [
  { value: "all", label: "За всё время" },
  { value: "hour", label: "Последний час" },
  { value: "day", label: "Последние сутки" },
  { value: "week", label: "Последняя неделя" },
];

/** Resolved against "now" at query time, not when the option was picked —
 * a modal left open for an hour would otherwise keep filtering by the hour
 * it was opened in. */
export function sinceMsFor(range: DateRangeOption): number | undefined {
  const now = Date.now();
  switch (range) {
    case "hour":
      return now - 60 * 60 * 1000;
    case "day":
      return now - 24 * 60 * 60 * 1000;
    case "week":
      return now - 7 * 24 * 60 * 60 * 1000;
    case "all":
      return undefined;
  }
}

/** The tool-call log: a filtered, paged view over what the assistant has
 * actually done, plus clearing it.
 *
 * Every filter change resets to the first page. Without that, narrowing a
 * filter could leave `offset` past the new total and show an empty page over
 * a non-empty result. */
export function useToolCallLog(projectRoot: string | null) {
  const [rows, setRows] = useState<ToolCallLogRow[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [clearing, setClearing] = useState(false);

  const [search, setSearch] = useState("");
  const [tool, setTool] = useState("");
  const [status, setStatus] = useState("");
  const [dateRange, setDateRange] = useState<DateRangeOption>("all");
  const [onlyCurrentRepo, setOnlyCurrentRepo] = useState(true);

  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const filter = useMemo(
    () => ({
      repoRoot: onlyCurrentRepo && projectRoot ? projectRoot : undefined,
      tool: tool || undefined,
      status: status || undefined,
      search: search.trim() || undefined,
      sinceMs: sinceMsFor(dateRange),
      limit: TOOL_CALL_LOG_PAGE_SIZE,
      offset,
    }),
    [onlyCurrentRepo, projectRoot, tool, status, search, dateRange, offset],
  );

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const page = await queryToolCallLog(filter);
      if (!mounted.current) return;
      setRows(page.rows);
      setTotal(page.total);
      setError(null);
    } catch (e) {
      if (mounted.current) setError(toMessage(e));
    } finally {
      if (mounted.current) setLoading(false);
    }
  }, [filter]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    setOffset(0);
  }, [onlyCurrentRepo, projectRoot, tool, status, search, dateRange]);

  const clearLog = useCallback(async () => {
    setClearing(true);
    try {
      await clearToolCallLog();
      await load();
      return true;
    } catch (e) {
      if (mounted.current) setError(toMessage(e));
      return false;
    } finally {
      if (mounted.current) setClearing(false);
    }
  }, [load]);

  const canPrev = offset > 0;
  const canNext = offset + rows.length < total;

  return {
    rows,
    total,
    offset,
    loading,
    error,
    clearing,
    search,
    setSearch,
    tool,
    setTool,
    status,
    setStatus,
    dateRange,
    setDateRange,
    onlyCurrentRepo,
    setOnlyCurrentRepo,
    canPrev,
    canNext,
    /** `"0 из 0"` when empty, `"1–50 из 214"` otherwise. */
    rangeLabel: total === 0 ? "0 из 0" : `${offset + 1}–${offset + rows.length} из ${total}`,
    prevPage: () => setOffset((o) => Math.max(0, o - TOOL_CALL_LOG_PAGE_SIZE)),
    nextPage: () => setOffset((o) => o + TOOL_CALL_LOG_PAGE_SIZE),
    clearLog,
  };
}
