import { useLlmSetup } from "../../hooks/useLlmSetup";

/** Assistant sound + OS-notification toggles — kept as their own Settings
 * section (next to Провайдеры / Разрешения) rather than mixed into the LLM
 * provider list, so the provider tab stays about credentials and models. */
export function NotificationsTab() {
  const {
    settings,
    busy,
    error,
    setTaskDoneSoundEnabled,
    setNeedAnswerSoundEnabled,
  } = useLlmSetup();

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

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
