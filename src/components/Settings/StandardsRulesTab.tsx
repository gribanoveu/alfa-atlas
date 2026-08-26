import { useStandardsRules } from "../../hooks/useStandardsRules";
import "../Welcome/CloneRepoModal.css";
import "./StandardsRulesTab.css";

export function StandardsRulesTab() {
  const { rules, config, error, busy, isEnabled, toggleRule } = useStandardsRules();

  return (
    <>
      <div className="settings-section-title">Стандарты API-документации</div>
      <p className="settings-lead">
        Правила проверки соответствия документации методов API корпоративному
        стандарту. Выключенное правило не участвует в подсчёте баллов.
      </p>
      <div className="standards-rules-list">
        {rules === null ? (
          <p className="settings-hint">Загрузка…</p>
        ) : (
          rules.map((rule) => (
            <div key={rule.id} className="standards-rule-row">
              <label className="settings-check">
                <input
                  type="checkbox"
                  checked={isEnabled(rule)}
                  disabled={!config || busy}
                  onChange={(event) => void toggleRule(rule, event.target.checked)}
                />
                <span className="standards-rule-id">{rule.id}</span>
                <span>{rule.title}</span>
              </label>
              <span className="standards-rule-weight">{rule.weight}</span>
            </div>
          ))
        )}
      </div>
      {error ? <div className="settings-error">{error}</div> : null}
    </>
  );
}
