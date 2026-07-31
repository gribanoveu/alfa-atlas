import { useCallback, useEffect, useState } from "react";
import {
  getSpellcheckConfig,
  type SpellcheckConfig,
} from "../lib/spellcheck";

const DEFAULT_SPELLCHECK_CONFIG: SpellcheckConfig = {
  enabled: true,
  dictionaries: {},
  skipCamelCase: true,
};

export function useSpellcheckConfig() {
  const [config, setConfig] = useState<SpellcheckConfig>(
    DEFAULT_SPELLCHECK_CONFIG,
  );
  const [ready, setReady] = useState(false);

  const reload = useCallback(async () => {
    try {
      const next = await getSpellcheckConfig();
      setConfig(next);
    } catch {
      setConfig(DEFAULT_SPELLCHECK_CONFIG);
    } finally {
      setReady(true);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  return { config, ready, reload, setConfig };
}
