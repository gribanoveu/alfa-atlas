import { useLlmSetup } from "../../hooks/useLlmSetup";

/** LLM request and tool-call logging — kept out of the Провайдеры tab so that
 * one stays about credentials and models. */
export function LoggingTab() {
  const { settings, busy, error, setDebugLogging, setToolCallLogging } = useLlmSetup();

  return (
    <div className="settings-sections">
      <div className="settings-card">
        <div className="settings-section-title">Запросы и ответы</div>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={settings?.debugLogging ?? false}
            disabled={busy || !settings}
            onChange={(event) => void setDebugLogging(event.target.checked)}
          />
          <span>Логировать запросы и ответы модели</span>
        </label>
        <p className="settings-hint">
          Записывает каждый запрос и ответ в{" "}
          <code>~/.atlas/logs/llm.jsonl</code>: раунды ассистента в чате
          (<code>round ≥ 1</code>), а также one-shot вызовы — «Сократить»/selection AI,
          compaction истории и авто-сжатие памяти (<code>round: 0</code>). Полезно, чтобы
          разобраться в ошибке провайдера. Выключено по умолчанию: переписка может
          содержать содержимое документов.
        </p>
      </div>

      <div className="settings-card">
        <div className="settings-section-title">Инструменты</div>
        <label className="settings-check">
          <input
            type="checkbox"
            checked={settings?.toolCallLogging ?? true}
            disabled={busy || !settings}
            onChange={(event) => void setToolCallLogging(event.target.checked)}
          />
          <span>Логировать вызовы инструментов</span>
        </label>
        <p className="settings-hint">
          Записывает каждый вызов инструмента ассистента (путь, статус, длительность) в{" "}
          <code>~/.atlas/tool_calls.db</code> — журнал можно посмотреть в Инструменты → Журнал
          вызовов инструментов. Включено по умолчанию: содержимое документов не сохраняется,
          только метаданные вызова.
        </p>
      </div>

      {error ? <div className="settings-error">{error}</div> : null}
    </div>
  );
}
