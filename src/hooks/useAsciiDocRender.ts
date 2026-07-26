import { useEffect, useState } from "react";
import { Extensions, load, MemoryLogger, LoggerManager } from "asciidoctor";
import type { Document } from "asciidoctor";
import { readProjectFile } from "../lib/project";

type RenderState = {
  doc: Document | null;
  error: string | null;
  parsing: boolean;
};

// Minimal shape of the IncludeProcessor DSL `this` context. The full
// `IncludeProcessorDslInterface` from asciidoctor's types is not resolvable
// through the package's `exports` field; we only need `handles` and `process`.
interface IncludeProcessorDsl {
  handles(fn: (target: string) => boolean): void;
  process(
    fn: (
      doc: unknown,
      reader: {
        lineno: number;
        pushInclude(
          data: string | string[],
          file?: string,
          path?: string,
          lineno?: number,
          attrs?: Record<string, unknown>,
        ): unknown;
      },
      target: string,
      attrs: Record<string, string>,
    ) => void | Promise<void>,
  ): void;
}

/**
 * Build a fresh extension registry whose IncludeProcessor reads included
 * files from disk via the `read_project_file` Tauri command. asciidoctor
 * itself handles the subsequent parsing/recursion of the pushed content.
 *
 * Must be per-parse: a module-global registry would close over a stale
 * `docsRoot`. When `docsRoot` is null we fall back to asciidoctor's default
 * include resolution (which will surface «Unresolved directive» for any
 * include — same as before this hook knew about docsRoot).
 */
function createFileIncludeRegistry(docsRoot: string) {
  const registry = Extensions.create();
  registry.includeProcessor(function (this: IncludeProcessorDsl) {
    this.handles(function (_target: string) {
      return true;
    });
    this.process(async function (_doc, reader, target, attrs) {
      try {
        const content = await readProjectFile(docsRoot, target);
        reader.pushInclude(content, target, target, 1, attrs);
      } catch {
        // Leave the include unresolved — asciidoctor will render the default
        // «Unresolved directive» placeholder. The parse error is also logged
        // by asciidoctor via the MemoryLogger installed by the caller.
      }
    });
  });
  return registry;
}

/**
 * Парсит AsciiDoc-контент в AST-документ (без вызова `convert()`).
 *
 * Отдельный от `useAsciiDocParser` путь: тот извлекает факты для индекса,
 * этот — строит дерево блоков для рендера превью. Дебаунс ~250мс, чтобы
 * перестройка не мешала набору.
 *
 * `docsRoot` нужен для раскрытия `include::file.adoc[]` — IncludeProcessor
 * читает включаемые файлы через backend-команду `read_project_file` с
 * валидацией пути против docsRoot.
 */
export function useAsciiDocRender(
  content: string,
  enabled: boolean,
  docsRoot: string | null,
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

      const loadOpts: Record<string, unknown> = {
        sourcemap: true,
        // `safe: "server"` разрешает include через IncludeProcessor (в отличие
        // от `safe: "safe"`, который их полностью блокирует). `unsafe` не
        // используем — это дало бы asciidoctor прямой доступ к файловой системе
        // без нашего контейнирования через docsRoot.
        safe: "server",
        attributes: { showtitle: true },
      };
      if (docsRoot) {
        loadOpts.extension_registry = createFileIncludeRegistry(docsRoot);
        // base_dir делает относительные `include::foo.adoc[]` резолвящимися
        // от docsRoot (asciidoctor использует его для диагностики и для
        // вложенных include, которые наш процессор делегирует обратно).
        loadOpts.base_dir = docsRoot;
      }

      load(content, loadOpts)
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
  }, [content, enabled, docsRoot]);

  return state;
}
