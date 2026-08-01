import { useCallback, useEffect, useState } from "react";
import { loadOpenApiBundle, type OpenApiBundleResult } from "../lib/openapi";

/** Lazily loads the fully-resolved OpenAPI bundle only once `enabled` is true
 * (i.e. the API Explorer tab has actually been opened), so the resolve cost
 * isn't paid on every project open. */
export function useOpenApiBundle(
  repoRoot: string | null,
  entryFile: string | null,
  enabled: boolean,
) {
  const [bundle, setBundle] = useState<OpenApiBundleResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!repoRoot || !entryFile) return;
    setLoading(true);
    try {
      const result = await loadOpenApiBundle(repoRoot, entryFile);
      setBundle(result);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [repoRoot, entryFile]);

  useEffect(() => {
    setBundle(null);
    setError(null);
  }, [repoRoot, entryFile]);

  useEffect(() => {
    if (enabled && !bundle && !loading) void load();
  }, [enabled, bundle, loading, load]);

  return { bundle, loading, error, reload: load };
}
