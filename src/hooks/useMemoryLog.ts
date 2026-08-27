import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import { deleteMemoryLogEntry, queryMemoryLog, type MemoryLogRow } from "../lib/memoryLog";

export const MEMORY_LOG_PAGE_SIZE = 50;

/** One row's identity across both stores — `id` alone collides, since the
 * project and global stores number their entries independently. */
export function memoryRowKey(row: Pick<MemoryLogRow, "scope" | "id">): string {
  return `${row.scope}-${row.id}`;
}

/** The agent's memory log: a filtered, paged view over both the project and
 * global stores, plus deleting single entries.
 *
 * Every filter change resets to the first page, so narrowing cannot leave
 * `offset` past the new total. */
export function useMemoryLog(projectRoot: string | null) {
  const [rows, setRows] = useState<MemoryLogRow[]>([]);
  const [total, setTotal] = useState(0);
  const [projectStorePath, setProjectStorePath] = useState<string | null>(null);
  const [globalStorePath, setGlobalStorePath] = useState("");
  const [offset, setOffset] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deletingKey, setDeletingKey] = useState<string | null>(null);

  const [search, setSearch] = useState("");
  const [scope, setScope] = useState("");

  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const filter = useMemo(
    () => ({
      scope: scope || undefined,
      search: search.trim() || undefined,
      repoRoot: projectRoot ?? undefined,
      limit: MEMORY_LOG_PAGE_SIZE,
      offset,
    }),
    [scope, search, projectRoot, offset],
  );

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const page = await queryMemoryLog(filter);
      if (!mounted.current) return;
      setRows(page.rows);
      setTotal(page.total);
      setProjectStorePath(page.projectStorePath);
      setGlobalStorePath(page.globalStorePath);
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
  }, [scope, search, projectRoot]);

  /** `true` when the entry was actually removed. A repeated call for a row
   * already being deleted is ignored rather than sending a second request. */
  const deleteEntry = useCallback(
    async (row: MemoryLogRow) => {
      const key = memoryRowKey(row);
      if (deletingKey === key) return false;
      setDeletingKey(key);
      try {
        await deleteMemoryLogEntry({
          scope: row.scope,
          id: row.id,
          // Only a project-scoped entry needs a repo to resolve its store.
          repoRoot: row.scope === "project" ? (projectRoot ?? undefined) : undefined,
        });
        await load();
        if (mounted.current) setError(null);
        return true;
      } catch (e) {
        if (mounted.current) setError(toMessage(e));
        return false;
      } finally {
        if (mounted.current) setDeletingKey(null);
      }
    },
    [deletingKey, load, projectRoot],
  );

  return {
    rows,
    total,
    offset,
    projectStorePath,
    globalStorePath,
    loading,
    error,
    deletingKey,
    search,
    setSearch,
    scope,
    setScope,
    canPrev: offset > 0,
    canNext: offset + rows.length < total,
    rangeLabel: total === 0 ? "0 из 0" : `${offset + 1}–${offset + rows.length} из ${total}`,
    prevPage: () => setOffset((o) => Math.max(0, o - MEMORY_LOG_PAGE_SIZE)),
    nextPage: () => setOffset((o) => o + MEMORY_LOG_PAGE_SIZE),
    deleteEntry,
  };
}
