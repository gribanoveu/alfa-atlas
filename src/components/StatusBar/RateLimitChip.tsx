import { useEffect, useRef, useState } from "react";
import type { RateLimitSeverity, RateLimitSnapshot } from "../../lib/llm";
import "./StatusBar.css";

type RateLimitChipProps = {
  snapshot: RateLimitSnapshot;
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

function formatK(n: number): string {
  if (n >= 1000) {
    const k = n / 1000;
    return Number.isInteger(k) ? `${k}k` : `${k.toFixed(1)}k`;
  }
  return String(n);
}

function spaced(n: number): string {
  return n.toLocaleString("ru-RU").replace(/\u00A0/g, " ");
}

function hhmm(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function dur(ms: number): string {
  if (ms <= 0) return "0 сек";
  const s = Math.ceil(ms / 1000);
  if (s < 60) return `${s} сек`;
  const m = Math.floor(s / 60);
  const rest = s % 60;
  return rest === 0 ? `${m} мин` : `${m} мин ${String(rest).padStart(2, "0")} сек`;
}

function severityClass(severity: RateLimitSeverity): string {
  switch (severity) {
    case "normal":
      return "normal";
    case "warning":
      return "warning";
    case "critical":
      return "critical";
    case "limited":
      return "limited";
    case "offHours":
      return "off-hours";
  }
}

export function RateLimitChip({ snapshot, open, onOpenChange }: RateLimitChipProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!open && !snapshot.isLimited) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [open, snapshot.isLimited]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        onOpenChange(false);
      }
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open, onOpenChange]);

  const chipLabel = !snapshot.isEnforced
    ? "без лимита"
    : `${formatK(snapshot.used)} / ${formatK(snapshot.limit)}`;

  const pct = snapshot.limit > 0 ? Math.min(100, (snapshot.used / snapshot.limit) * 100) : 0;
  const hint = !snapshot.isEnforced
    ? snapshot.nextEnforceAt != null
      ? `Лимит с ${hhmm(snapshot.nextEnforceAt)} · через ${dur(snapshot.nextEnforceAt - now)}`
      : "Лимит не проверяется"
    : snapshot.isLimited && snapshot.retryUntil != null
      ? `Повтор с ${hhmm(snapshot.retryUntil)} · через ${dur(snapshot.retryUntil - now)}`
      : snapshot.nextReleaseAt != null
        ? `Освободится ${formatK(snapshot.releases[0]?.tokens ?? 0)} · через ${dur(snapshot.nextReleaseAt - now)}`
        : "Окно пустое";

  return (
    <div className="seg rate-limit" ref={rootRef}>
      <button
        type="button"
        className={`seg rate-limit-chip clickable ${severityClass(snapshot.severity)}${open ? " open" : ""}`}
        onClick={() => onOpenChange(!open)}
        title={`${snapshot.label} API — лимит completion-токенов`}
        aria-expanded={open}
        aria-haspopup="dialog"
      >
        <span
          className={`rate-limit-dot${snapshot.isLimited ? " pulse" : ""}`}
          aria-hidden
        />
        <span className="rate-limit-name">{snapshot.label}</span>
        <span className="rate-limit-value">{chipLabel}</span>
      </button>

      {open ? (
        <div className="rate-limit-popover" role="dialog" aria-labelledby="rate-limit-popover-title">
          <div className="rate-limit-title" id="rate-limit-popover-title">
            Лимиты API
          </div>
          <div className="rate-limit-usage-row">
            <span className="rate-limit-usage-num">
              {spaced(snapshot.used)}
              <span className="rate-limit-usage-den"> / {spaced(snapshot.limit)}</span>
            </span>
            <span className="rate-limit-usage-unit">
              {snapshot.isEnforced ? `${spaced(snapshot.remaining)} осталось` : "08:00–21:00"}
            </span>
          </div>
          <div className="rate-limit-bar-track">
            <div
              className={`rate-limit-bar-fill ${severityClass(snapshot.severity)}`}
              style={{ width: `${snapshot.isEnforced ? pct : 0}%` }}
            />
          </div>
          <div className={`rate-limit-hint${snapshot.isLimited ? " limited" : ""}`}>{hint}</div>
        </div>
      ) : null}
    </div>
  );
}
