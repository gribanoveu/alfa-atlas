import { useCallback, useEffect, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import {
  getStandardsConfig,
  getStandardsRules,
  setStandardsConfig,
  type RuleDef,
  type StandardsRuleConfig,
} from "../lib/standards";

/** The API-documentation standards rules and which of them are enabled.
 *
 * Toggles are optimistic and roll back to whatever the backend holds if the
 * write fails, so a checkbox never stays flipped on a setting that was not
 * saved. A rule the config says nothing about falls back to its own
 * `defaultEnabled` rather than to `false` — a newly shipped rule is on until
 * the user turns it off. */
export function useStandardsRules() {
  const [rules, setRules] = useState<RuleDef[] | null>(null);
  const [config, setConfig] = useState<StandardsRuleConfig | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const [nextRules, nextConfig] = await Promise.all([
          getStandardsRules(),
          getStandardsConfig(),
        ]);
        if (!mounted.current) return;
        setRules(nextRules);
        setConfig(nextConfig);
        setError(null);
      } catch (e) {
        if (mounted.current) setError(toMessage(e));
      }
    })();
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
        if (mounted.current) setError(null);
      } catch (e) {
        if (!mounted.current) return;
        setError(toMessage(e));
        const current = await getStandardsConfig().catch(() => config);
        if (current && mounted.current) setConfig(current);
      } finally {
        if (mounted.current) setBusy(false);
      }
    },
    [config],
  );

  return { rules, config, error, busy, isEnabled, toggleRule };
}
