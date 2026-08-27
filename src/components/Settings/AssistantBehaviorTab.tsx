import { useLlmSetup } from "../../hooks/useLlmSetup";

/** How the assistant behaves around the conversation itself — long-term
 * memory and the prompts shown above the thread. Kept apart from
 * `LlmTab` (credentials and models) and `LoggingTab` (diagnostics). */
export function AssistantBehaviorTab() {
  const {
    settings,
    busy,
    error,
    setMemoryExtractionEnabled,
    setFollowUpSuggestionsDisabled,
  } = useLlmSetup();

  return (
    <div className="settings-sections">
      <div className="settings-card">
        <div className="settings-section-title">Память</div>
        <p className="settings-hint settings-hint-compact">
          После каждого ответа отдельный вызов модели извлекает долговечные факты
          и записывает их в память. Основной агент об этом не знает — задержка
          ответа не зависит от записи.
        </p>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={settings?.memoryExtractionEnabled ?? true}
            disabled={busy || !settings}
            onChange={(event) => void setMemoryExtractionEnabled(event.target.checked)}
          />
          <span>Извлекать факты в память после ответа</span>
        </label>
        <p className="settings-hint">
          Выключите, если не хотите, чтобы диалоги пополняли долгосрочную память.
          Уже сохранённые факты по-прежнему подмешиваются в контекст.
        </p>
      </div>

      <div className="settings-card">
        <div className="settings-section-title">Подсказки в чате</div>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={settings?.followUpSuggestionsDisabled ?? false}
            disabled={busy || !settings}
            onChange={(event) =>
              void setFollowUpSuggestionsDisabled(event.target.checked)
            }
          />
          <span>Отключить подсказки после выбора сценария</span>
        </label>
        <p className="settings-hint">
          Скрывает подсказки над перепиской, которые появляются после выбора
          одного из стартовых предложений в новом чате. Стартовые предложения в
          пустом чате остаются в любом случае.
        </p>
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
