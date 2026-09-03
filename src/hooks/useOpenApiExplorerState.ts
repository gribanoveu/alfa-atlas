import { useCallback, useEffect, useMemo, useState } from "react";
import type { AuthValue, AuthValues } from "../components/OpenApiExplorer/security";
import type { ExecutedResponse } from "../components/OpenApiExplorer/requestExecutor";
import type { ParamValues } from "../components/OpenApiExplorer/requestBuilder";

export type TryItOutForm = {
  paramValues: ParamValues;
  bodyText: string;
};

export type TryRun = {
  at: number;
  method: string;
  url: string;
  response: ExecutedResponse | null;
  error: string | null;
};

/** Сколько последних запусков хранить на операцию. Достаточно, чтобы сравнить
 * «до/после» правки параметра, и мало настолько, чтобы не копить тела ответов. */
const HISTORY_LIMIT = 5;

export function operationKey(path: string, method: string): string {
  return `${method} ${path}`;
}

/**
 * Состояние вида API Explorer, поднятое из самого компонента в App: вкладка
 * Explorer'а — псевдовкладка, её компонент размонтируется при каждом закрытии,
 * и вместе с ним раньше пропадали выбранная операция, заполненная форма
 * запроса и введённые секреты. Здесь всё это живёт до смены проекта.
 *
 * Секреты по-прежнему только в памяти: на диск ничего не пишется, при закрытии
 * проекта (`repoRoot`) состояние сбрасывается целиком.
 */
export function useOpenApiExplorerState(repoRoot: string | null) {
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<{ path: string; method: string } | null>(null);
  const [collapsedTags, setCollapsedTags] = useState<ReadonlySet<string>>(new Set());
  const [authValues, setAuthValues] = useState<AuthValues>({});
  const [authOpen, setAuthOpen] = useState(false);
  const [baseUrlOverride, setBaseUrlOverride] = useState<string | null>(null);
  const [forms, setForms] = useState<Record<string, TryItOutForm>>({});
  const [history, setHistory] = useState<Record<string, TryRun[]>>({});
  const [tryOpen, setTryOpen] = useState<ReadonlySet<string>>(new Set());

  useEffect(() => {
    setFilter("");
    setSelected(null);
    setCollapsedTags(new Set());
    setAuthValues({});
    setAuthOpen(false);
    setBaseUrlOverride(null);
    setForms({});
    setHistory({});
    setTryOpen(new Set());
  }, [repoRoot]);

  const toggleTag = useCallback((tag: string) => {
    setCollapsedTags((prev) => {
      const next = new Set(prev);
      if (next.has(tag)) next.delete(tag);
      else next.add(tag);
      return next;
    });
  }, []);

  const setAllTagsCollapsed = useCallback((tags: string[], collapsed: boolean) => {
    setCollapsedTags(collapsed ? new Set(tags) : new Set());
  }, []);

  const setAuthValue = useCallback((schemeId: string, value: AuthValue) => {
    setAuthValues((prev) => ({ ...prev, [schemeId]: value }));
  }, []);

  const clearAuth = useCallback(() => setAuthValues({}), []);

  const setForm = useCallback((key: string, form: TryItOutForm) => {
    setForms((prev) => ({ ...prev, [key]: form }));
  }, []);

  const pushRun = useCallback((key: string, run: TryRun) => {
    setHistory((prev) => ({
      ...prev,
      [key]: [run, ...(prev[key] ?? [])].slice(0, HISTORY_LIMIT),
    }));
  }, []);

  const toggleTryOpen = useCallback((key: string) => {
    setTryOpen((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  return useMemo(
    () => ({
      filter,
      setFilter,
      selected,
      setSelected,
      collapsedTags,
      toggleTag,
      setAllTagsCollapsed,
      authValues,
      setAuthValue,
      clearAuth,
      authOpen,
      setAuthOpen,
      baseUrlOverride,
      setBaseUrlOverride,
      forms,
      setForm,
      history,
      pushRun,
      tryOpen,
      toggleTryOpen,
    }),
    [
      filter,
      selected,
      collapsedTags,
      toggleTag,
      setAllTagsCollapsed,
      authValues,
      setAuthValue,
      clearAuth,
      authOpen,
      baseUrlOverride,
      forms,
      setForm,
      history,
      pushRun,
      tryOpen,
      toggleTryOpen,
    ],
  );
}

export type OpenApiExplorerState = ReturnType<typeof useOpenApiExplorerState>;
