import { useCallback, useEffect, useRef, useState } from "react";
import {
  checkStandards,
  type StandardsReport,
} from "../lib/standards";

export type StandardsCheckStatus = "idle" | "running" | "done" | "error";

type UseStandardsCheckOptions = {
  active: boolean;
};

export function useStandardsCheck(
  docsRoot: string | null,
  { active }: UseStandardsCheckOptions,
) {
  const [report, setReport] = useState<StandardsReport | null>(null);
  const [status, setStatus] = useState<StandardsCheckStatus>("idle");
  const [error, setError] = useState<string | null>(null);
  const runIdRef = useRef(0);

  // A different project was opened — drop the stale report rather than
  // showing results for a repo that's no longer open.
  useEffect(() => {
    runIdRef.current += 1;
    setReport(null);
    setStatus("idle");
    setError(null);
  }, [docsRoot]);

  const runCheck = useCallback(async () => {
    if (!active || !docsRoot) return;
    const runId = ++runIdRef.current;
    setStatus("running");
    setError(null);
    try {
      const result = await checkStandards(docsRoot);
      if (runIdRef.current !== runId) return;
      setReport(result);
      setStatus("done");
    } catch (err) {
      if (runIdRef.current !== runId) return;
      setError(err instanceof Error ? err.message : String(err));
      setStatus("error");
    }
  }, [active, docsRoot]);

  return { report, status, error, runCheck };
}
