import { useCallback, useEffect, useState } from "react";
import { artifactDelete, artifactList, type ArtifactSummary } from "../lib/artifacts";
import { toMessage } from "../lib/errors";

/** The repository's saved artifacts, for the artifacts list. The builder
 *  itself does not use this — it owns one record and loads it directly, the
 *  same split `usePlans`/`PlanDetailView` already use. */
export function useArtifacts(enabled: boolean) {
  const [artifacts, setArtifacts] = useState<ArtifactSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setArtifacts(await artifactList());
    } catch (e) {
      setError(toMessage(e));
      setArtifacts([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!enabled) return;
    void reload();
  }, [enabled, reload]);

  const remove = useCallback(
    async (artifactId: string) => {
      try {
        await artifactDelete(artifactId);
        setArtifacts((prev) => prev.filter((a) => a.id !== artifactId));
      } catch (e) {
        setError(toMessage(e));
      }
    },
    [],
  );

  return { artifacts, loading, error, reload, remove };
}
