import { useEffect, useRef, useState } from "react";

/** Live seconds-elapsed counter, ticking once a second while `running` is
 * true (same `setInterval`+`Date.now()` pattern as `RateLimitChip`). Once
 * `running` flips `false` it latches the last computed value and stops
 * ticking, so the caller can freeze a final duration in place instead of it
 * drifting from later unrelated re-renders. */
export function useElapsedSeconds(startedAt: number, running: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  const frozenRef = useRef<number | null>(null);

  useEffect(() => {
    if (!running) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [running]);

  const seconds = Math.floor((now - startedAt) / 1000);
  if (!running) {
    if (frozenRef.current === null) frozenRef.current = seconds;
    return frozenRef.current;
  }
  frozenRef.current = null;
  return seconds;
}

/** Russian spelled-out duration, matching `RateLimitChip`'s own `dur()`
 * (e.g. "5 сек", "1 мин", "1 мин 20 сек") rather than compact "5s"/"1m20s"
 * shorthand — kept consistent with the rest of the app's Russian UI. */
export function formatElapsedDuration(seconds: number): string {
  if (seconds <= 0) return "0 сек";
  if (seconds < 60) return `${seconds} сек`;
  const m = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return rest === 0 ? `${m} мин` : `${m} мин ${rest} сек`;
}
