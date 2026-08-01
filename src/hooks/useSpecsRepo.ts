import { useEffect, useState } from "react";
import { detectSpecsRepo, type SpecsRepoInfo } from "../lib/openapi";

/** Detects whether `repoRoot` follows the `specs/{schemas,responses,parameters,operations}`
 * OpenAPI convention. Runs once per `repoRoot` change, independent of `docsRoot`. */
export function useSpecsRepo(repoRoot: string | null) {
  const [info, setInfo] = useState<SpecsRepoInfo | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!repoRoot) {
      setInfo(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    detectSpecsRepo(repoRoot)
      .then((result) => {
        if (!cancelled) setInfo(result);
      })
      .catch(() => {
        if (!cancelled) setInfo(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [repoRoot]);

  return { info, loading };
}
