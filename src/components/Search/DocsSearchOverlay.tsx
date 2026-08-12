import { useEffect, useMemo, useRef } from "react";
import { CaseSensitive, Regex, Search, X } from "lucide-react";
import type { GrepMatch } from "../../lib/aiTools";
import { useDocsSearch } from "../../hooks/useDocsSearch";
import "./DocsSearchOverlay.css";

type DocsSearchOverlayProps = {
  open: boolean;
  docsRoot: string | null;
  onClose: () => void;
  onOpenHit: (path: string, line: number) => void;
};

type MatchGroup = {
  path: string;
  items: GrepMatch[];
};

function groupByPath(matches: GrepMatch[]): MatchGroup[] {
  const map = new Map<string, GrepMatch[]>();
  for (const m of matches) {
    const list = map.get(m.path);
    if (list) list.push(m);
    else map.set(m.path, [m]);
  }
  return [...map.entries()].map(([path, items]) => ({ path, items }));
}

export function DocsSearchOverlay({
  open,
  docsRoot,
  onClose,
  onOpenHit,
}: DocsSearchOverlayProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const wasOpenRef = useRef(false);
  const search = useDocsSearch(open ? docsRoot : null);

  useEffect(() => {
    if (open && !wasOpenRef.current) {
      const t = window.setTimeout(() => inputRef.current?.focus(), 0);
      wasOpenRef.current = true;
      return () => window.clearTimeout(t);
    }
    if (!open && wasOpenRef.current) {
      search.reset();
      wasOpenRef.current = false;
    }
    // Reset / focus only on open→closed transitions.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [open, onClose]);

  const groups = useMemo(
    () => (search.results ? groupByPath(search.results.matches) : []),
    [search.results],
  );

  if (!open) return null;

  const queryTrimmed = search.query.trim();
  let empty: string | null = null;
  if (!docsRoot) {
    empty = "Откройте проект, чтобы искать в документации";
  } else if (!queryTrimmed) {
    empty = "Введите запрос";
  } else if (!search.loading && search.results && search.results.matches.length === 0) {
    empty = "Ничего не найдено";
  }

  return (
    <div
      className="docs-search-backdrop"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="docs-search-panel"
        role="dialog"
        aria-modal="true"
        aria-label="Поиск в документации"
      >
        <div className="docs-search-head">
          <Search size={16} className="docs-search-head-icon" aria-hidden />
          <span className="docs-search-title">Найти в документации</span>
          <button
            type="button"
            className="docs-search-close"
            onClick={onClose}
            aria-label="Закрыть"
          >
            <X size={16} aria-hidden />
          </button>
        </div>

        <div className="docs-search-fields">
          <div className="docs-search-query-row">
            <input
              ref={inputRef}
              type="search"
              className="docs-search-input"
              placeholder="Поиск…"
              value={search.query}
              onChange={(e) => search.setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  search.searchNow();
                }
              }}
              aria-label="Текст поиска"
              spellCheck={false}
              autoComplete="off"
            />
            <button
              type="button"
              className={`docs-search-toggle${search.matchCase ? " is-active" : ""}`}
              title="С учётом регистра"
              aria-pressed={search.matchCase}
              aria-label="С учётом регистра"
              onClick={() => search.setMatchCase(!search.matchCase)}
            >
              <CaseSensitive size={15} aria-hidden />
            </button>
            <button
              type="button"
              className={`docs-search-toggle${search.useRegex ? " is-active" : ""}`}
              title="Регулярное выражение"
              aria-pressed={search.useRegex}
              aria-label="Регулярное выражение"
              onClick={() => search.setUseRegex(!search.useRegex)}
            >
              <Regex size={15} aria-hidden />
            </button>
          </div>
          <input
            type="text"
            className="docs-search-glob"
            placeholder="Файлы для включения, например *.adoc"
            value={search.glob}
            onChange={(e) => search.setGlob(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                search.searchNow();
              }
            }}
            aria-label="Фильтр файлов (glob)"
            spellCheck={false}
            autoComplete="off"
          />
        </div>

        <div className="docs-search-body">
          {search.error ? (
            <div className="docs-search-error" role="alert">
              {search.error}
            </div>
          ) : null}
          {search.loading && !search.results ? (
            <div className="panel-empty">Поиск…</div>
          ) : empty !== null ? (
            <div className="panel-empty">{empty}</div>
          ) : (
            <>
              {search.results?.truncated ? (
                <div className="docs-search-truncated" role="status">
                  Показаны первые {search.results.matches.length} совпадений
                </div>
              ) : null}
              {groups.map((group) => (
                <div key={group.path} className="docs-search-file-group">
                  <div className="docs-search-file-header" title={group.path}>
                    <span className="docs-search-file-name">{group.path}</span>
                    <span className="docs-search-file-count">{group.items.length}</span>
                  </div>
                  <ul className="docs-search-list">
                    {group.items.map((m, i) => (
                      <li key={`${m.path}:${m.line}:${i}`}>
                        <button
                          type="button"
                          className="docs-search-item"
                          onClick={() => onOpenHit(m.path, m.line)}
                          title={`${m.path}:${m.line}`}
                        >
                          <span className="docs-search-item-loc">{m.line}</span>
                          <span className="docs-search-item-text">{m.text}</span>
                        </button>
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
