import { useRef } from "react";
import { formatElapsedDuration, useElapsedSeconds } from "../../hooks/useElapsedSeconds";

type AssistantElapsedTimerProps = {
  running: boolean;
  className?: string;
};

/** Ticking "Nс/N мин" label anchored to this component's own mount instant
 * — the caller mounts it exactly when the phase it's timing begins (e.g.
 * the first reasoning delta), so no `startedAt` prop is needed. */
export function AssistantElapsedTimer({ running, className }: AssistantElapsedTimerProps) {
  const startedAtRef = useRef(Date.now());
  const seconds = useElapsedSeconds(startedAtRef.current, running);
  if (seconds < 1) return null;
  return (
    <span className={`assistant-elapsed-timer${className ? ` ${className}` : ""}`}>
      {formatElapsedDuration(seconds)}
    </span>
  );
}
