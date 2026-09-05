import { useEffect, useState } from "react";
import { AlertTriangle, CalendarDays, RefreshCw, Settings2 } from "lucide-react";
import { useCalendarEvents } from "../../hooks/useCalendarEvents";
import { toMessage } from "../../lib/errors";
import {
  forgetCalendarPassword,
  getCalendarSettings,
  saveCalendarSettings,
  setCalendarPassword,
  type CalendarEvent,
  type CalendarSettingsView,
} from "../../lib/calendar";
import { CalendarTimeline, SelectedEvent } from "./CalendarTimeline";
import "./CalendarPanel.css";

export type CalendarPanelProps = {
  onOpenSettings: () => void;
};

/** OWA/Exchange calendar as a per-day timeline. The cache and sync live in
 * Rust, so the panel stays warm across dock switches; positioning is done in
 * the viewer's display zone while the backend stays UTC. */
export function CalendarPanel({ onOpenSettings }: CalendarPanelProps) {
  const { events, status, loading, error, refresh, reloadStatus } = useCalendarEvents();
  const [view, setView] = useState<CalendarSettingsView | null>(null);
  // Selected meeting for the bottom sheet — held here (not in the timeline) so
  // the sheet can overlay the panel instead of pushing the grid down.
  const [selected, setSelected] = useState<CalendarEvent | null>(null);

  // Settings the panel needs: the display zone (times are shown in it; backend
  // stays UTC) and the login, pre-filled into the password prompt and saved
  // there like in Settings.
  const loadSettings = () => {
    void getCalendarSettings()
      .then(setView)
      .catch(() => setView(null));
  };
  useEffect(loadSettings, []);

  const tz = view?.settings.displayTimeZone ?? "";
  const configured = status?.configured ?? false;
  const connected = status?.connected ?? false;

  return (
    <div className="cal-panel">
      <div className="cal-panel-toolbar">
        <span className="cal-panel-toolbar-title">Календарь</span>
        <div className="cal-panel-toolbar-actions">
          <button
            type="button"
            className="cal-panel-icon-btn"
            disabled={loading || !connected}
            title="Обновить"
            aria-label="Обновить"
            onClick={() => void refresh()}
          >
            <RefreshCw className={loading ? "spin" : undefined} size={13} aria-hidden />
          </button>
          <button
            type="button"
            className="cal-panel-icon-btn"
            title="Настройки календаря"
            aria-label="Настройки календаря"
            onClick={onOpenSettings}
          >
            <Settings2 size={13} aria-hidden />
          </button>
        </div>
      </div>

      <div className="cal-panel-body">
        {status === null ? (
          <p className="cal-panel-status">Проверяем соединение…</p>
        ) : !configured ? (
          <div className="cal-panel-notice">
            <CalendarDays className="cal-panel-notice-icon" size={16} aria-hidden />
            <p className="cal-panel-notice-text">Календарь не настроен.</p>
            <button type="button" className="cal-panel-btn" onClick={onOpenSettings}>
              Настроить календарь
            </button>
          </div>
        ) : !connected ? (
          <PasswordPrompt
            circuitOpen={status.circuitOpen}
            initialUsername={view?.settings.username ?? ""}
            onSubmit={async (username, password, remember) => {
              // Persist the login like Settings does, so it stays pre-filled.
              if (view && username.trim() !== view.settings.username) {
                await saveCalendarSettings({ ...view.settings, username: username.trim() });
              }
              await setCalendarPassword(password, remember);
              loadSettings();
              await refresh();
            }}
          />
        ) : (
          <>
            {status?.circuitOpen ? (
              <div className="cal-panel-notice is-error">
                <AlertTriangle className="cal-panel-notice-icon" size={16} aria-hidden />
                <p className="cal-panel-notice-text">
                  {error ?? "Слишком много неудачных входов."}
                </p>
                <button
                  type="button"
                  className="cal-panel-btn"
                  onClick={async () => {
                    setSelected(null);
                    await forgetCalendarPassword();
                    await reloadStatus();
                  }}
                >
                  Выйти
                </button>
              </div>
            ) : error ? (
              <div className="cal-panel-notice is-error">
                <AlertTriangle className="cal-panel-notice-icon" size={16} aria-hidden />
                <p className="cal-panel-notice-text">{error}</p>
              </div>
            ) : null}
            <CalendarTimeline
              events={events}
              tz={tz}
              selectedId={selected?.id ?? null}
              onSelect={(ev) => setSelected((cur) => (ev && cur?.id === ev.id ? null : ev))}
            />
          </>
        )}
      </div>

      {connected && selected ? (
        <SelectedEvent
          event={selected}
          tz={tz}
          onClose={() => setSelected(null)}
          onRsvpDone={() => void refresh()}
        />
      ) : null}
    </div>
  );
}

function PasswordPrompt({
  circuitOpen,
  initialUsername,
  onSubmit,
}: {
  circuitOpen: boolean;
  initialUsername: string;
  onSubmit: (username: string, password: string, remember: boolean) => Promise<void>;
}) {
  const [username, setUsername] = useState(initialUsername);
  const [password, setPassword] = useState("");
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // Keep in sync if settings load after the first render.
  useEffect(() => {
    setUsername(initialUsername);
  }, [initialUsername]);

  const submit = async () => {
    if (!username.trim() || !password || busy) return;
    setBusy(true);
    setErr(null);
    try {
      await onSubmit(username, password, remember);
      setPassword("");
    } catch (e) {
      setErr(toMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="cal-form">
      {circuitOpen ? (
        <p className="cal-form-warn">
          Слишком много неудачных входов. Проверьте пароль и войдите заново.
        </p>
      ) : (
        <p className="cal-form-intro">Введите доменный логин и пароль</p>
      )}

      <label className="cal-form-field">
        <span className="cal-form-label">Логин</span>
        <input
          className="cal-input"
          type="text"
          value={username}
          placeholder={"ДОМЕН\\логин"}
          autoComplete="off"
          autoCapitalize="off"
          spellCheck={false}
          disabled={busy}
          onChange={(e) => setUsername(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void submit();
          }}
        />
      </label>

      <label className="cal-form-field">
        <span className="cal-form-label">Пароль</span>
        <input
          className="cal-input"
          type="password"
          value={password}
          placeholder="Доменный пароль"
          autoComplete="off"
          disabled={busy}
          onChange={(e) => setPassword(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void submit();
          }}
        />
      </label>

      <label className="cal-check">
        <input
          type="checkbox"
          checked={remember}
          disabled={busy}
          onChange={(e) => setRemember(e.target.checked)}
        />
        <span>Запомнить пароль на этом компьютере</span>
      </label>
      {remember ? (
        <p className="cal-form-hint">
          Пароль сохранится в зашифрованном виде.
        </p>
      ) : null}

      {err ? <p className="cal-form-error">{err}</p> : null}
      <button
        type="button"
        className="cal-submit"
        disabled={!username.trim() || !password || busy}
        onClick={() => void submit()}
      >
        {busy ? "Подключение…" : "Войти"}
      </button>
    </div>
  );
}
