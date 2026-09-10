import { useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { toMessage } from "../../lib/errors";
import {
  forgetCalendarPassword,
  getCalendarSettings,
  saveCalendarSettings,
  type CalendarSettings,
  type CalendarSettingsView,
} from "../../lib/calendar";
import { CERT_PLACEHOLDER } from "./certField";
import "./CalendarTab.css";

/** Settings form. Text fields commit on blur (whole-file `settings.json`
 * rewrite, and a URL mid-typing is momentarily invalid), so there is no
 * separate Save button. The domain password is not
 * entered here (that is the panel, where "remember" is decided); this tab only
 * shows whether one is stored and offers to forget it. */
export function CalendarTab() {
  const [view, setView] = useState<CalendarSettingsView | null>(null);
  const [baseUrl, setBaseUrl] = useState("");
  const [username, setUsername] = useState("");
  const [tz, setTz] = useState("");
  const [pem, setPem] = useState("");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // What the backend last confirmed, so a blur on an untouched field skips a
  // no-op rewrite.
  const saved = useRef<CalendarSettings | null>(null);

  const adopt = (v: CalendarSettingsView) => {
    setView(v);
    setBaseUrl(v.settings.baseUrl);
    setUsername(v.settings.username);
    setTz(v.settings.displayTimeZone);
    setPem(v.settings.trustedCertPem ?? "");
    saved.current = v.settings;
  };

  const load = async () => {
    try {
      adopt(await getCalendarSettings());
    } catch (e) {
      setError(toMessage(e));
    }
  };

  useEffect(() => {
    void load();
  }, []);

  /** Writes the current field values, unless nothing changed. Rolls back to
   * what the backend really holds on failure. */
  const commit = async () => {
    if (!view || busy) return;
    const next: CalendarSettings = {
      baseUrl: baseUrl.trim().replace(/\/+$/, ""),
      username: username.trim(),
      displayTimeZone: tz.trim(),
      rememberPassword: view.settings.rememberPassword,
      trustedCertPem: pem.trim() ? pem : null,
      // Owned by the notifications tab; carried through untouched.
      reminderMinutes: view.settings.reminderMinutes,
    };
    if (saved.current && shallowEqual(saved.current, next)) return;
    setBusy(true);
    setError(null);
    try {
      await saveCalendarSettings(next);
      await load();
    } catch (e) {
      setError(toMessage(e));
      await load(); // reflect the real stored state
    } finally {
      setBusy(false);
    }
  };

  const forget = async () => {
    setBusy(true);
    try {
      await forgetCalendarPassword();
      await load();
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setBusy(false);
    }
  };

  if (!view) {
    return (
      <div className="settings-sections calendar-tab">
        {error ? <div className="settings-error">{error}</div> : <p>Загрузка...</p>}
      </div>
    );
  }

  return (
    <div className="settings-sections calendar-tab">
      <div className="settings-card">
        <div className="settings-section-title">Подключение</div>
        <p className="settings-hint settings-hint-compact">
          Приложение авторизуется на сервере OWA/Exchange по доменному логину и паролю (NTLM).
          Пароль вводится в панели календаря, а не здесь.
        </p>

        <label className="cal-field">
          <span className="cal-field-label">Адрес OWA/Exchange</span>
          <input
            className="clone-modal-input"
            type="text"
            placeholder={view.bundledBaseUrl ?? "https://owa.example.com"}
            value={baseUrl}
            disabled={busy}
            onChange={(e) => setBaseUrl(e.target.value)}
            onBlur={() => void commit()}
          />
          <p className="settings-hint settings-hint-compact">
            {view.bundledBaseUrl
              ? `Адрес задан сборкой (${view.bundledBaseUrl}). Заполните, чтобы использовать другой сервер.`
              : "Только корень сервера, без /owa/."}
          </p>
        </label>

        <label className="cal-field">
          <span className="cal-field-label">Логин</span>
          <input
            className="clone-modal-input"
            type="text"
            placeholder="DOMAIN\username"
            value={username}
            disabled={busy}
            onChange={(e) => setUsername(e.target.value)}
            onBlur={() => void commit()}
          />
          <p className="settings-hint settings-hint-compact">{"В формате ДОМЕН\\логин, например MOSCOW\\ivanov."}</p>
        </label>

        <label className="cal-field">
          <span className="cal-field-label">Часовой пояс отображения</span>
          <input
            className="clone-modal-input"
            type="text"
            placeholder="Europe/Moscow (по умолчанию — системный)"
            value={tz}
            disabled={busy}
            onChange={(e) => setTz(e.target.value)}
            onBlur={() => void commit()}
          />
          <p className="settings-hint settings-hint-compact">
            IANA-имя пояса. Пустое поле — пояс этого компьютера. Влияет только на показ времени.
          </p>
        </label>

        <div className="cal-field">
          <span className="cal-field-label">Пароль</span>
          <p className="settings-hint settings-hint-compact">
            {view.hasPassword
              ? "Пароль сохранён на этом компьютере в зашифрованном виде."
              : "Пароль не сохранён — вводится в панели календаря."}
          </p>
          {view.hasPassword ? (
            <button type="button" className="settings-btn" disabled={busy} onClick={() => void forget()}>
              Забыть пароль
            </button>
          ) : null}
        </div>
      </div>

      <div className="settings-card cal-advanced">
        <button type="button" className="cal-advanced-toggle" onClick={() => setAdvancedOpen((o) => !o)}>
          {advancedOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          Дополнительно
        </button>
        {advancedOpen ? (
          <div className="cal-advanced-body">
          <label className="cal-field">
            <span className="cal-field-label">Корневой сертификат (PEM)</span>
            <textarea
              className="clone-modal-input cal-cert"
              placeholder={view.hasBundledCert ? "Задан сборкой приложения" : CERT_PLACEHOLDER}
              value={pem}
              disabled={busy}
              onChange={(e) => setPem(e.target.value)}
              onBlur={() => void commit()}
            />
            <p className="settings-hint settings-hint-compact">
              Нужен для внутреннего сервера за корпоративным УЦ. Заменяет публичные корни. Пусто —
              используется сертификат сборки, если он есть.
            </p>
          </label>
          </div>
        ) : null}
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}

function shallowEqual(a: CalendarSettings, b: CalendarSettings): boolean {
  return (
    a.baseUrl === b.baseUrl &&
    a.username === b.username &&
    a.displayTimeZone === b.displayTimeZone &&
    a.rememberPassword === b.rememberPassword &&
    a.reminderMinutes === b.reminderMinutes &&
    (a.trustedCertPem ?? null) === (b.trustedCertPem ?? null)
  );
}
