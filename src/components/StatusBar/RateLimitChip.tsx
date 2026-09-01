import { useEffect, useRef, useState } from "react";
import type {
  RateLimitResource,
  RateLimitResourceKind,
  RateLimitSeverity,
  RateLimitSnapshot,
} from "../../lib/llm";
import "./StatusBar.css";

/** Wording for the three counters. Lives here rather than in the manifest:
 * the backend sends a `kind`, the labels are UI copy. */
const RESOURCE_LABELS: Record<RateLimitResourceKind, string> = {
  prompt: "Запрос",
  completion: "Ответ",
  requests: "Обращения",
};

/** Unit for the "frees up" hint — tokens for the two token counters, calls
 * for the request counter. */
const RESOURCE_UNITS: Record<RateLimitResourceKind, string> = {
  prompt: "токенов",
  completion: "токенов",
  requests: "обращений",
};

type RateLimitChipProps = {
  snapshot: RateLimitSnapshot;
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

/** Compact enough for the chip, which now has to fit a 10 000 000 prompt
 * cap next to a 1 000 request one — hence the M step, or the token counters
 * would read "10000k". */
function formatK(n: number): string {
  if (n >= 1_000_000) {
    const m = n / 1_000_000;
    return Number.isInteger(m) ? `${m}M` : `${m.toFixed(1)}M`;
  }
  if (n >= 1000) {
    const k = n / 1000;
    return Number.isInteger(k) ? `${k}k` : `${k.toFixed(1)}k`;
  }
  return String(n);
}

function spaced(n: number): string {
  return n.toLocaleString("ru-RU").replace(/\u00A0/g, " ");
}

/** Exact digits where they fit, compact where they don't: the popover is
 * 280px wide and "9 900 000 / 10 000 000" wraps onto three lines, while
 * "60 000" and "1 000" read better in full. */
function exactOrCompact(n: number): string {
  return n >= 1_000_000 ? formatK(n) : spaced(n);
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

  // The chip has room for one number, so it shows whichever counter the
  // backend nominated as closest to its cap.
  const drivingLabel = snapshot.drivingKind ? RESOURCE_LABELS[snapshot.drivingKind] : "";
  const chipLabel = !snapshot.isEnforced
    ? "без лимита"
    : `${formatK(snapshot.used)} / ${formatK(snapshot.limit)}`;

  const driving = snapshot.resources.find((r) => r.kind === snapshot.drivingKind) ?? null;
  const hint = !snapshot.isEnforced
    ? snapshot.nextEnforceAt != null
      ? `Лимит с ${hhmm(snapshot.nextEnforceAt)} · через ${dur(snapshot.nextEnforceAt - now)}`
      : "Лимит не проверяется"
    : snapshot.isLimited && snapshot.retryUntil != null
      ? `Повтор с ${hhmm(snapshot.retryUntil)} · через ${dur(snapshot.retryUntil - now)}`
      : driving?.nextReleaseAt != null
        ? `Освободится ${formatK(driving.nextReleaseAmount)} ${
            RESOURCE_UNITS[driving.kind]
          } · через ${dur(driving.nextReleaseAt - now)}`
        : "Окно пустое";

  const windowLabel =
    snapshot.windowMs != null ? `окно ${Math.round(snapshot.windowMs / 60000)} мин` : "";
  const limitedCount = snapshot.resources.filter((r) => r.isLimited).length;

  return (
    <div className="seg rate-limit" ref={rootRef}>
      <button
        type="button"
        className={`seg rate-limit-chip clickable ${severityClass(snapshot.severity)}${open ? " open" : ""}`}
        onClick={() => onOpenChange(!open)}
        title={
          snapshot.isEnforced && drivingLabel
            ? `${snapshot.label} API — ближе всего к лимиту: ${drivingLabel.toLowerCase()}`
            : `${snapshot.label} API — лимиты запроса, ответа и числа обращений`
        }
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
            Лимиты API{windowLabel ? <span className="rate-limit-window"> · {windowLabel}</span> : null}
          </div>

          {snapshot.resources.map((resource) => (
            <ResourceRow
              key={resource.kind}
              resource={resource}
              isEnforced={snapshot.isEnforced}
              isDriving={resource.kind === snapshot.drivingKind}
              // Общая подсказка внизу уже называет самый поздний срок
              // повтора; расписывать сроки по строкам стоит только когда
              // упёрлись сразу в несколько счётчиков и они разные.
              showRetry={limitedCount > 1}
              now={now}
            />
          ))}

          <div className={`rate-limit-hint${snapshot.isLimited ? " limited" : ""}`}>{hint}</div>
          {snapshot.offHoursOverride ? (
            <div className="rate-limit-hint">
              Сейчас нерабочее время — сервер лимиты не проверяет, но подсчёт включён в настройках.
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

/** One counter: name, numbers, bar. The driving one is marked so the chip's
 * single number can be traced back to the row it came from. */
function ResourceRow({
  resource,
  isEnforced,
  isDriving,
  showRetry,
  now,
}: {
  resource: RateLimitResource;
  isEnforced: boolean;
  isDriving: boolean;
  showRetry: boolean;
  now: number;
}) {
  const pct = resource.limit > 0 ? Math.min(100, (resource.used / resource.limit) * 100) : 0;
  return (
    <div className={`rate-limit-resource${isDriving ? " is-driving" : ""}`}>
      <div className="rate-limit-usage-row">
        <span className="rate-limit-usage-num">
          <span className="rate-limit-resource-name">{RESOURCE_LABELS[resource.kind]}</span>
          {exactOrCompact(resource.used)}
          <span className="rate-limit-usage-den"> / {exactOrCompact(resource.limit)}</span>
        </span>
        {/* Вне рабочего времени остаток не значит ничего, а «с 09:00» уже
            сказано подсказкой внизу — три повтора подряд только шумят. */}
        <span className="rate-limit-usage-unit">
          {isEnforced ? `${exactOrCompact(resource.remaining)} осталось` : ""}
        </span>
      </div>
      <div className="rate-limit-bar-track">
        <div
          className={`rate-limit-bar-fill ${severityClass(resource.severity)}`}
          style={{ width: `${isEnforced ? pct : 0}%` }}
        />
      </div>
      {showRetry && isEnforced && resource.isLimited && resource.retryUntil != null ? (
        <div className="rate-limit-resource-note">
          освободится {hhmm(resource.retryUntil)} · через {dur(resource.retryUntil - now)}
        </div>
      ) : null}
    </div>
  );
}
