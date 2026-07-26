import { useEffect, useState } from "react";
import { load, MemoryLogger, LoggerManager } from "asciidoctor";
import type { Document } from "asciidoctor";

type RenderState = {
  doc: Document | null;
  error: string | null;
  parsing: boolean;
};

/**
 * Парсит AsciiDoc-контент в AST-документ (без вызова `convert()`).
 *
 * Отдельный от `useAsciiDocParser` путь: тот извлекает факты для индекса,
 * этот — строит дерево блоков для рендера превью. Дебаунс ~250мс, чтобы
 * перестройка не мешала набору.
 */
export function useAsciiDocRender(
  content: string,
  enabled: boolean,
): RenderState {
  const [state, setState] = useState<RenderState>({
    doc: null,
    error: null,
    parsing: false,
  });

  useEffect(() => {
    if (!enabled) {
      setState({ doc: null, error: null, parsing: false });
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(() => {
      setState((prev) => ({ ...prev, parsing: true }));

      const logger = new MemoryLogger();
      const previousLogger = LoggerManager.getLogger();
      LoggerManager.setLogger(logger);

      load(content, {
        sourcemap: true,
        safe: "safe",
        attributes: { showtitle: true },
      })
        .then((doc) => {
          if (cancelled) return;
          setState({ doc, error: null, parsing: false });
        })
        .catch((e: unknown) => {
          if (cancelled) return;
          setState({
            doc: null,
            error: e instanceof Error ? e.message : String(e),
            parsing: false,
          });
        })
        .finally(() => {
          LoggerManager.setLogger(previousLogger);
        });
    }, 250);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [content, enabled]);

  return state;
}
