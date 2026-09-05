import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, ChevronDown, ChevronLeft, ChevronRight, Copy, Users, Video } from "lucide-react";
import { toMessage } from "../../lib/errors";
import {
  calendarEventDetails,
  calendarRsvp,
  formatDayMonth,
  formatEventTime,
  formatWeekday,
  isEffectivelyCancelled,
  tzDayKey,
  tzHourFraction,
  type CalendarEvent,
  type EventDetails,
  type MeetingResponseType,
  type RsvpAction,
} from "../../lib/calendar";
import "./CalendarTimeline.css";

const PX_PER_HOUR = 56;
const MIN_BLOCK_PX = 22;
/** Below this height a block puts time + title on one line instead of two. */
const COMPACT_BELOW_PX = 40;

type Positioned = {
  ev: CalendarEvent;
  startH: number;
  endH: number;
  lane: number;
  lanes: number;
};

/** A vertical day timeline: one day at a time, ‹ / › to move between days,
 * events placed by time with overlaps split into lanes, and a "now" line on
 * today. Meetings expand to details on click. Backend stays UTC — all
 * positioning is done in the viewer's display zone via `Intl`. */
export function CalendarTimeline({
  events,
  tz,
  selectedId,
  onSelect,
}: {
  events: CalendarEvent[];
  tz: string;
  selectedId: string | null;
  /** Toggle selection: called with the clicked event, or null on navigation. */
  onSelect: (event: CalendarEvent | null) => void;
}) {
  const [dayOffset, setDayOffset] = useState(0);

  const rep = new Date(Date.now() + dayOffset * 86_400_000);
  const dayKey = tzDayKey(rep.toISOString(), tz);

  // Events touching this day (in the display zone), clamped to [0, 24].
  const forDay = events
    .filter((ev) => {
      const s = tzDayKey(ev.start, tz);
      const e = tzDayKey(ev.end, tz);
      return s <= dayKey && dayKey <= e;
    })
    .map((ev) => {
      const sameStart = tzDayKey(ev.start, tz) === dayKey;
      const sameEnd = tzDayKey(ev.end, tz) === dayKey;
      const startH = sameStart ? tzHourFraction(ev.start, tz) : 0;
      const endH = sameEnd ? Math.max(tzHourFraction(ev.end, tz), startH) : 24;
      return { ev, startH, endH };
    });

  const allDay = forDay.filter((x) => x.ev.isAllDay || x.endH - x.startH >= 24);
  const timed = forDay.filter((x) => !(x.ev.isAllDay || x.endH - x.startH >= 24));

  // Visible hour window: always 8–20, widened to fit the day's events.
  let viewStart = 8;
  let viewEnd = 20;
  for (const t of timed) {
    viewStart = Math.min(viewStart, Math.floor(t.startH));
    viewEnd = Math.max(viewEnd, Math.ceil(t.endH));
  }
  viewStart = Math.max(0, viewStart);
  viewEnd = Math.min(24, Math.max(viewEnd, viewStart + 1));

  const laid = layoutLanes(timed);
  const gridHeight = (viewEnd - viewStart) * PX_PER_HOUR;

  const isToday = dayOffset === 0;
  const nowH = tzHourFraction(new Date().toISOString(), tz);
  const showNow = isToday && nowH >= viewStart && nowH <= viewEnd;

  const hours: number[] = [];
  for (let h = viewStart; h <= viewEnd; h++) hours.push(h);

  // Top line is always present (stable header height): the relative name for
  // adjacent days, otherwise the weekday.
  const iso = rep.toISOString();
  const topLine =
    dayOffset === 0
      ? "Сегодня"
      : dayOffset === 1
        ? "Завтра"
        : dayOffset === -1
          ? "Вчера"
          : formatWeekday(iso, tz);

  return (
    <div className="cal-tl">
      <div className="cal-tl-nav">
        <button
          type="button"
          className="cal-tl-arrow"
          aria-label="Предыдущий день"
          onClick={() => {
            onSelect(null);
            setDayOffset((d) => d - 1);
          }}
        >
          <ChevronLeft size={15} aria-hidden />
        </button>
        <button
          type="button"
          className={`cal-tl-date${isToday ? " is-today" : ""}`}
          title="К сегодняшнему дню"
          onClick={() => {
            onSelect(null);
            setDayOffset(0);
          }}
        >
          <span className="cal-tl-rel">{topLine}</span>
          <span className="cal-tl-datefull">{formatDayMonth(iso, tz)}</span>
        </button>
        <button
          type="button"
          className="cal-tl-arrow"
          aria-label="Следующий день"
          onClick={() => {
            onSelect(null);
            setDayOffset((d) => d + 1);
          }}
        >
          <ChevronRight size={15} aria-hidden />
        </button>
      </div>


      {allDay.length > 0 ? (
        <div className="cal-tl-allday">
          {allDay.map((x) => (
            <button
              key={x.ev.id}
              type="button"
              className={`cal-tl-allday-item${isEffectivelyCancelled(x.ev) ? " is-cancelled" : ""}`}
              title={x.ev.title}
              onClick={() => onSelect(x.ev)}
            >
              {x.ev.title}
            </button>
          ))}
        </div>
      ) : null}

      <div className="cal-tl-grid" style={{ height: `${gridHeight}px` }}>
        {hours.map((h) => (
          <div key={h} className="cal-tl-hour" style={{ top: `${(h - viewStart) * PX_PER_HOUR}px` }}>
            <span className="cal-tl-hour-label">{String(h).padStart(2, "0")}:00</span>
            <span className="cal-tl-hour-line" />
          </div>
        ))}

        <div className="cal-tl-track">
          {laid.map((p) => {
            const cancelled = isEffectivelyCancelled(p.ev);
            const canJoin = !cancelled && Boolean(p.ev.joinUrl);
            const top = (p.startH - viewStart) * PX_PER_HOUR;
            const height = Math.max((p.endH - p.startH) * PX_PER_HOUR - 3, MIN_BLOCK_PX);
            const compact = height < COMPACT_BELOW_PX;
            const width = 100 / p.lanes;
            return (
              <button
                key={p.ev.id}
                type="button"
                className={
                  `cal-tl-event resp-${p.ev.responseType}` +
                  (cancelled ? " is-cancelled" : "") +
                  (compact ? " is-compact" : "") +
                  (canJoin ? " is-join" : "") +
                  (selectedId === p.ev.id ? " is-selected" : "")
                }
                style={{
                  top: `${top}px`,
                  height: `${height}px`,
                  left: `${p.lane * width}%`,
                  width: `calc(${width}% - 3px)`,
                }}
                title={p.ev.title}
                onClick={() => onSelect(p.ev)}
              >
                <span className="cal-tl-event-time">
                  {formatEventTime(p.ev.start, tz)}
                </span>
                <span className="cal-tl-event-title">{p.ev.title}</span>
                {canJoin ? (
                  <span
                    className="cal-tl-event-join"
                    role="button"
                    tabIndex={0}
                    title="Войти"
                    aria-label="Войти во встречу"
                    onClick={(e) => {
                      e.stopPropagation();
                      if (p.ev.joinUrl) void openUrl(p.ev.joinUrl);
                    }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && p.ev.joinUrl) void openUrl(p.ev.joinUrl);
                    }}
                  >
                    <Video size={12} aria-hidden />
                  </span>
                ) : null}
              </button>
            );
          })}

          {showNow ? (
            <div className="cal-tl-now" style={{ top: `${(nowH - viewStart) * PX_PER_HOUR}px` }}>
              <span className="cal-tl-now-dot" />
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

/** Greedy lane assignment: overlapping events are split into side-by-side
 * lanes, each cluster of mutually-overlapping events sized to its own lane
 * count so a single busy hour doesn't narrow the whole day. */
function layoutLanes(items: { ev: CalendarEvent; startH: number; endH: number }[]): Positioned[] {
  const sorted = [...items].sort((a, b) => a.startH - b.startH || a.endH - b.endH);
  const out: Positioned[] = [];
  let cluster: Positioned[] = [];
  let laneEnds: number[] = [];

  const flush = () => {
    const lanes = laneEnds.length || 1;
    for (const p of cluster) p.lanes = lanes;
    out.push(...cluster);
    cluster = [];
    laneEnds = [];
  };

  for (const it of sorted) {
    if (laneEnds.length > 0 && it.startH >= Math.max(...laneEnds)) flush();
    let lane = laneEnds.findIndex((end) => end <= it.startH);
    if (lane === -1) {
      lane = laneEnds.length;
      laneEnds.push(it.endH);
    } else {
      laneEnds[lane] = it.endH;
    }
    cluster.push({ ...it, lane, lanes: 1 });
  }
  flush();
  return out;
}

export function SelectedEvent({
  event,
  tz,
  onClose,
  onRsvpDone,
}: {
  event: CalendarEvent | null;
  tz: string;
  onClose: () => void;
  /** Called after a successful RSVP so the panel can refresh from the cache. */
  onRsvpDone?: () => void;
}) {
  const [details, setDetails] = useState<EventDetails | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [shown, setShown] = useState(false);
  const [copied, setCopied] = useState(false);
  const [closing, setClosing] = useState(false);
  const [rsvpBusy, setRsvpBusy] = useState<RsvpAction | null>(null);
  const [rsvpErr, setRsvpErr] = useState<string | null>(null);
  const [localResponse, setLocalResponse] = useState<MeetingResponseType | null>(null);

  // Reset when a different event is opened. Details are NOT fetched here —
  // only when the user asks (one network round trip per meeting is not spent
  // unless they want the attendees/description).
  useEffect(() => {
    setDetails(null);
    setErr(null);
    setLoading(false);
    setShown(false);
    setCopied(false);
    setClosing(false);
    setRsvpBusy(null);
    setRsvpErr(null);
    setLocalResponse(null);
  }, [event?.id]);

  if (!event) return null;
  const cancelled = isEffectivelyCancelled(event);
  const canJoin = !cancelled && Boolean(event.joinUrl);
  // An invitation the viewer can answer: not their own meeting, not cancelled,
  // and with a change key (required by EWS).
  const isInvite = !cancelled && !event.isOrganizer && Boolean(event.changeKey);
  const currentResponse = localResponse ?? event.responseType;

  // Play the slide-out before unmounting.
  const requestClose = () => {
    setClosing(true);
    setTimeout(onClose, 170);
  };

  const rsvp = (action: RsvpAction, send: boolean) => {
    if (!event.changeKey || rsvpBusy) return;
    setRsvpBusy(action);
    setRsvpErr(null);
    calendarRsvp(event.id, event.changeKey, action, send)
      .then(() => {
        setLocalResponse(
          action === "accept" ? "accepted" : action === "tentative" ? "tentative" : "declined",
        );
        onRsvpDone?.();
      })
      .catch((e) => setRsvpErr(toMessage(e)))
      .finally(() => setRsvpBusy(null));
  };

  // First press fetches and shows; afterwards it just toggles visibility of the
  // already-loaded data — no second round trip.
  const toggleDetails = () => {
    if (!details) {
      if (loading) return;
      setLoading(true);
      setErr(null);
      setShown(true);
      calendarEventDetails(event.id, event.changeKey)
        .then(setDetails)
        .catch((e) => setErr(toMessage(e)))
        .finally(() => setLoading(false));
    } else {
      setShown((s) => !s);
    }
  };

  const copyLink = async () => {
    if (!event.joinUrl) return;
    try {
      await writeText(event.joinUrl);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard denied — nothing actionable to show */
    }
  };

  return (
    <div className={`cal-sel${closing ? " is-closing" : ""}`}>
      <div className="cal-sel-handle" aria-hidden />

      <div className="cal-sel-head">
        <span className={`cal-sel-title${cancelled ? " is-cancelled" : ""}`}>{event.title}</span>
        <button type="button" className="cal-sel-close" aria-label="Закрыть" onClick={requestClose}>
          ✕
        </button>
      </div>

      <div className="cal-sel-meta">
        <div className="cal-sel-time">
          {formatEventTime(event.start, tz)}–{formatEventTime(event.end, tz)}
        </div>
        {event.organizer ? <div className="cal-sel-org">{event.organizer}</div> : null}
        {event.location ? <div className="cal-sel-loc">{event.location}</div> : null}
      </div>

      {canJoin ? (
        <div className="cal-sel-actions">
          <button type="button" className="cal-sel-join" onClick={() => void openUrl(event.joinUrl!)}>
            <Video size={14} aria-hidden /> Войти
          </button>
          <button
            type="button"
            className="cal-sel-copy"
            title="Скопировать ссылку"
            onClick={() => void copyLink()}
          >
            {copied ? <Check size={14} aria-hidden /> : <Copy size={14} aria-hidden />}
            {copied ? "Скопировано" : "Ссылка"}
          </button>
        </div>
      ) : null}

      {isInvite ? (
        <div className="cal-sel-section cal-sel-rsvp">
          <span className="cal-sel-section-label">Ваш ответ</span>
          <div className="cal-sel-rsvp-row">
            <RsvpSplit
              action="accept"
              label="Принять"
              active={currentResponse === "accepted"}
              busy={rsvpBusy}
              onPick={rsvp}
            />
            <RsvpSplit
              action="tentative"
              label="Под вопросом"
              active={currentResponse === "tentative"}
              busy={rsvpBusy}
              onPick={rsvp}
            />
            <RsvpSplit
              action="decline"
              label="Отклонить"
              active={currentResponse === "declined"}
              busy={rsvpBusy}
              onPick={rsvp}
            />
          </div>
          {rsvpErr ? <p className="cal-form-error">{rsvpErr}</p> : null}
        </div>
      ) : null}

      {/* Details (attendees + description) are fetched only on request, and
          can be hidden again without re-fetching. */}
      <button
        type="button"
        className="cal-sel-load"
        disabled={loading}
        aria-expanded={shown && Boolean(details)}
        onClick={toggleDetails}
      >
        <Users size={13} aria-hidden />
        {loading
          ? "Загрузка…"
          : err
            ? "Повторить"
            : details && shown
              ? "Скрыть участников"
              : "Показать участников"}
      </button>
      {err ? <p className="cal-form-error">{err}</p> : null}

      {details && shown ? (
        <>
          {details.attendees.length > 0 ? (
            <div className="cal-sel-attendees">
              <span className="cal-sel-attendees-count">Участники: {details.attendees.length}</span>
              <ul className="cal-attendees">
                {details.attendees.map((a, i) => (
                  <li key={`${a.email ?? a.name}-${i}`} className="cal-attendee">
                    <span className={`cal-attendee-dot resp-${a.response}`} aria-hidden />
                    <span className="cal-attendee-info">
                      <span className="cal-attendee-name">
                        {a.name}
                        {!a.required ? <span className="cal-attendee-opt"> · необяз.</span> : null}
                      </span>
                      {a.email ? <span className="cal-attendee-email">{a.email}</span> : null}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            <p className="cal-event-detail-status">Участников нет.</p>
          )}
          {details.bodyText ? <p className="cal-event-body">{details.bodyText}</p> : null}
        </>
      ) : null}
    </div>
  );
}

/** Split button for one RSVP action: the main press answers *with* a reply to
 * the organizer; the ▾ opens a hand-rolled menu (no native `<select>`, per the
 * app's UI rules) with the silent "без ответа" variant. */
function RsvpSplit({
  action,
  label,
  active,
  busy,
  onPick,
}: {
  action: RsvpAction;
  label: string;
  active: boolean;
  busy: RsvpAction | null;
  onPick: (action: RsvpAction, send: boolean) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const disabled = busy !== null;
  return (
    <div className={`cal-rsvp-split${active ? " is-active" : ""} ${action}`} ref={ref}>
      <button
        type="button"
        className={`cal-rsvp cal-rsvp-main ${action}${active ? " is-active" : ""}`}
        disabled={disabled}
        onClick={() => onPick(action, true)}
      >
        {busy === action ? "…" : label}
      </button>
      <button
        type="button"
        className="cal-rsvp cal-rsvp-caret"
        disabled={disabled}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="Другие варианты ответа"
        onClick={() => setOpen((o) => !o)}
      >
        <ChevronDown size={13} aria-hidden />
      </button>
      {open ? (
        <div className="cal-rsvp-menu" role="menu">
          <button
            type="button"
            role="menuitem"
            className="cal-rsvp-menu-item"
            onClick={() => {
              setOpen(false);
              onPick(action, false);
            }}
          >
            {label} без ответа
          </button>
        </div>
      ) : null}
    </div>
  );
}
