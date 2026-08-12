import { useCallback, useEffect, useRef, useState } from "react";
import { docsSearch, type DocsSearchResults } from "../lib/search";

const DEBOUNCE_MS = 300;

export type DocsSearchState = {
  query: string;
  setQuery: (value: string) => void;
  matchCase: boolean;
  setMatchCase: (value: boolean) => void;
  useRegex: boolean;
  setUseRegex: (value: boolean) => void;
  glob: string;
  setGlob: (value: string) => void;
  results: DocsSearchResults | null;
  loading: boolean;
  error: string | null;
  searchNow: () => void;
  reset: () => void;
};

function escapeRegex(literal: string): string {
  return literal.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function useDocsSearch(docsRoot: string | null): DocsSearchState {
  const [query, setQuery] = useState("");
  const [matchCase, setMatchCase] = useState(false);
  const [useRegex, setUseRegex] = useState(false);
  const [glob, setGlob] = useState("");
  const [results, setResults] = useState<DocsSearchResults | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const requestIdRef = useRef(0);
  const docsRootRef = useRef(docsRoot);
  docsRootRef.current = docsRoot;

  const runSearch = useCallback(async (rawQuery: string) => {
    const root = docsRootRef.current;
    const trimmed = rawQuery.trim();
    if (!root || !trimmed) {
      setResults(null);
      setError(null);
      setLoading(false);
      return;
    }

    const pattern = useRegex ? trimmed : escapeRegex(trimmed);
    const id = ++requestIdRef.current;
    setLoading(true);
    setError(null);
    try {
      const payload = await docsSearch(root, {
        pattern,
        glob: glob.trim() || null,
        caseInsensitive: !matchCase,
        maxResults: null,
      });
      if (id !== requestIdRef.current) return;
      setResults(payload);
    } catch (e) {
      if (id !== requestIdRef.current) return;
      setResults(null);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (id === requestIdRef.current) setLoading(false);
    }
  }, [matchCase, useRegex, glob]);

  const searchNow = useCallback(() => {
    void runSearch(query);
  }, [query, runSearch]);

  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed || !docsRoot) {
      setResults(null);
      setError(null);
      setLoading(false);
      return;
    }
    const timer = setTimeout(() => {
      void runSearch(query);
    }, DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query, docsRoot, runSearch]);

  const reset = useCallback(() => {
    requestIdRef.current += 1;
    setQuery("");
    setGlob("");
    setResults(null);
    setError(null);
    setLoading(false);
  }, []);

  return {
    query,
    setQuery,
    matchCase,
    setMatchCase,
    useRegex,
    setUseRegex,
    glob,
    setGlob,
    results,
    loading,
    error,
    searchNow,
    reset,
  };
}
