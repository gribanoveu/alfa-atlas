import { useEffect, useState } from "react";
import { useLlmSetup } from "../../hooks/useLlmSetup";
import { toMessage } from "../../lib/errors";
import { getCalendarSettings, saveCalendarSettings } from "../../lib/calendar";

/** Mirrors `domain::calendar::DEFAULT_REMINDER_MINUTES` — the lead time the
 * checkbox restores when it is switched back on. */
const DEFAULT_REMINDER_MINUTES = 5;
const MAX_REMINDER_MINUTES = 120;

/** Sound + OS-notification toggles — kept as their own Settings section (next
 * to Провайдеры / Разрешения) rather than mixed into the LLM provider list, so
 * the provider tab stays about credentials and models. The calendar reminder
 * lives here too even though it is stored under the calendar's own settings:
 * the user looks for notifications where the other notifications are. */
export function NotificationsTab() {
  const {
    settings,
    busy,
    error,
    setTaskDoneSoundEnabled,
    setNeedAnswerSoundEnabled,
  } = useLlmSetup();

  const [reminder, setReminder] = useState<number | null>(null);
  const [calBusy, setCalBusy] = useState(false);
  const [calError, setCalError] = useState<string | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        setReminder((await getCalendarSettings()).settings.reminderMinutes);
      } catch (e) {
        setCalError(toMessage(e));
      }
    })();
  }, []);

  /** Re-reads the stored calendar settings first: the calendar tab writes the
   * same whole-object file, so a snapshot from mount could undo an edit made
   * there in the meantime. */
  const saveReminder = async (minutes: number) => {
    const next = Math.min(Math.max(Math.round(minutes), 0), MAX_REMINDER_MINUTES);
    setReminder(next);
    setCalBusy(true);
    setCalError(null);
    try {
      const current = (await getCalendarSettings()).settings;
      await saveCalendarSettings({ ...current, reminderMinutes: next });
    } catch (e) {
      setCalError(toMessage(e));
      try {
        setReminder((await getCalendarSettings()).settings.reminderMinutes);
      } catch {
        /* leave the field as typed; the error above says it did not stick */
      }
    } finally {
      setCalBusy(false);
    }
  };

  return (
    <div className="settings-sections">
      <div className="settings-card">
        <div className="settings-section-title">Завершение работы</div>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={settings?.taskDoneSoundEnabled ?? true}
            disabled={busy || !settings}
            onChange={(event) => void setTaskDoneSoundEnabled(event.target.checked)}
          />
          <span>Уведомление при завершении работы агента</span>
        </label>
        <p className="settings-hint">
          Звук и системное уведомление, когда ассистент закончил ход. Не срабатывает при
          остановке пользователем или ошибке.
        </p>
      </div>

      <div className="settings-card">
        <div className="settings-section-title">Вопрос агента</div>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={settings?.needAnswerSoundEnabled ?? true}
            disabled={busy || !settings}
            onChange={(event) => void setNeedAnswerSoundEnabled(event.target.checked)}
          />
          <span>Уведомление, когда агент задаёт вопрос</span>
        </label>
        <p className="settings-hint">
          Звук и системное уведомление, когда появляется карточка с уточняющим вопросом.
          Обычные запросы на подтверждение инструментов остаются без уведомлений.
        </p>
      </div>

      <div className="settings-card">
        <div className="settings-section-title">Встречи в календаре</div>
        <div className="settings-section-head">
          <label className="settings-check">
            <input
              type="checkbox"
              checked={(reminder ?? 0) > 0}
              disabled={calBusy || reminder === null}
              onChange={(event) =>
                void saveReminder(event.target.checked ? DEFAULT_REMINDER_MINUTES : 0)
              }
            />
            <span>Напоминать о начале встречи</span>
          </label>

          {(reminder ?? 0) > 0 ? (
            <label className="settings-lead">
              за{" "}
              <input
                className="settings-number"
                type="number"
                min={1}
                max={MAX_REMINDER_MINUTES}
                step={1}
                value={reminder ?? DEFAULT_REMINDER_MINUTES}
                disabled={calBusy}
                onChange={(event) => setReminder(Number(event.target.value))}
                onBlur={(event) => void saveReminder(Number(event.target.value) || 1)}
              />{" "}
              мин
            </label>
          ) : null}
        </div>
        <p className="settings-hint">
          Звук и системное уведомление перед началом встречи из календаря. Отменённые,
          отклонённые и встречи на весь день не напоминают.
        </p>
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
      {calError ? <div className="settings-error">{calError}</div> : null}
    </div>
  );
}
