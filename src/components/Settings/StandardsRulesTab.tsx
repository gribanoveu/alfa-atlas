import { useCallback, useEffect, useState } from "react";
import {
  getStandardsConfig,
  getStandardsRules,
  setStandardsConfig,
  type RuleDef,
  type StandardsRuleConfig,
} from "../../lib/standards";
import "../Welcome/CloneRepoModal.css";
import "./StandardsRulesTab.css";

export function StandardsRulesTab() {
  const [rules, setRules] = useState<RuleDef[] | null>(null);
  const [config, setConfig] = useState<StandardsRuleConfig | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [nextRules, nextConfig] = await Promise.all([
          getStandardsRules(),
          getStandardsConfig(),
        ]);
        if (!cancelled) {
          setRules(nextRules);
          setConfig(nextConfig);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) {
          setError(e instanceof Error ? e.message : String(e));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const isEnabled = useCallback(
    (rule: RuleDef) => config?.rules[rule.id] ?? rule.defaultEnabled,
    [config],
  );

  const toggleRule = useCallback(
    async (rule: RuleDef, enabled: boolean) => {
      if (!config) return;
      const next: StandardsRuleConfig = {
        rules: { ...config.rules, [rule.id]: enabled },
      };
      setConfig(next);
      setBusy(true);
      try {
        await setStandardsConfig(next);
        setError(null);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        const current = await getStandardsConfig().catch(() => config);
        if (current) setConfig(current);
      } finally {
        setBusy(false);
      }
    },
    [config],
  );

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
                  disabled={!config || busy || rule.requiresNetwork}
                  onChange={(event) => void toggleRule(rule, event.target.checked)}
                />
                <span className="standards-rule-id">{rule.id}</span>
                <span>{rule.title}</span>
              </label>
              <span className="standards-rule-weight">{rule.weight}</span>
              {rule.requiresNetwork ? (
                <span className="standards-rule-note">требует сети — не реализовано</span>
              ) : null}
            </div>
          ))
        )}
      </div>
      {error ? <div className="settings-error">{error}</div> : null}
    </>
  );
}
