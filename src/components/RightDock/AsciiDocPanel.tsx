import { ChevronDown, ChevronRight, Search } from "lucide-react";
import { useMemo, useState } from "react";
import {
  ASCIIDOC_SNIPPET_CATEGORIES,
  filterSnippets,
  type AsciiDocSnippet,
  type AsciiDocSnippetCategory,
} from "../../lib/asciidocSnippets";
import { SnippetThumbnail } from "./SnippetThumbnail";
import "./AsciiDocPanel.css";

type AsciiDocPanelProps = {
  canInsert: boolean;
  onInsert: (text: string) => void;
};

type CategoryGroupProps = {
  label: string;
  count: number;
  open: boolean;
  onToggle: () => void;
  children: React.ReactNode;
};

function CategoryGroup({
  label,
  count,
  open,
  onToggle,
  children,
}: CategoryGroupProps) {
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <section className="adoc-group">
      <div className="adoc-group-head">
        <button
          type="button"
          className="adoc-group-toggle"
          onClick={onToggle}
          aria-expanded={open}
        >
          <Chevron className="adoc-group-chevron" size={14} aria-hidden />
          <span className="adoc-group-title">
            {label}
            <span className="adoc-group-count">({count})</span>
          </span>
        </button>
      </div>
      {open ? <div className="adoc-group-body">{children}</div> : null}
    </section>
  );
}

type SnippetCardProps = {
  snippet: AsciiDocSnippet;
  disabled: boolean;
  previewEnabled: boolean;
  onInsert: (text: string) => void;
};

function SnippetCard({
  snippet,
  disabled,
  previewEnabled,
  onInsert,
}: SnippetCardProps) {
  return (
    <button
      type="button"
      className="adoc-snippet-card"
      disabled={disabled}
      onClick={() => onInsert(snippet.template)}
      title={snippet.description ?? snippet.label}
      aria-label={`Вставить: ${snippet.label}`}
    >
      {previewEnabled ? <SnippetThumbnail snippet={snippet} /> : null}
      <span className="adoc-snippet-label">{snippet.label}</span>
    </button>
  );
}

export function AsciiDocPanel({ canInsert, onInsert }: AsciiDocPanelProps) {
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<Set<AsciiDocSnippetCategory>>(
    () => new Set(),
  );

  const filtered = useMemo(() => filterSnippets(query), [query]);

  const grouped = useMemo(() => {
    const map = new Map<AsciiDocSnippetCategory, AsciiDocSnippet[]>();
    for (const cat of ASCIIDOC_SNIPPET_CATEGORIES) {
      map.set(cat.id, []);
    }
    for (const snippet of filtered) {
      map.get(snippet.category)?.push(snippet);
    }
    return ASCIIDOC_SNIPPET_CATEGORIES.map((cat) => ({
      ...cat,
      snippets: map.get(cat.id) ?? [],
    })).filter((cat) => cat.snippets.length > 0);
  }, [filtered]);

  const toggleCategory = (id: AsciiDocSnippetCategory) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  return (
    <div className="adoc-panel">
      <div className="adoc-panel-search">
        <Search className="adoc-panel-search-icon" size={14} aria-hidden />
        <input
          type="search"
          className="adoc-panel-search-input"
          placeholder="Поиск блока…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Поиск блока AsciiDoc"
        />
      </div>

      {!canInsert ? (
        <div className="adoc-panel-banner" role="status">
          Вставка доступна только для AsciiDoc-файлов
        </div>
      ) : null}

      <div className="adoc-panel-scroll">
        {grouped.length === 0 ? (
          <div className="panel-empty">Блоки не найдены</div>
        ) : (
          grouped.map((cat) => {
            const open = !collapsed.has(cat.id);
            return (
              <CategoryGroup
                key={cat.id}
                label={cat.label}
                count={cat.snippets.length}
                open={open}
                onToggle={() => toggleCategory(cat.id)}
              >
                <div className="adoc-snippet-grid">
                  {cat.snippets.map((snippet) => (
                    <SnippetCard
                      key={snippet.id}
                      snippet={snippet}
                      disabled={!canInsert}
                      previewEnabled={open}
                      onInsert={onInsert}
                    />
                  ))}
                </div>
              </CategoryGroup>
            );
          })
        )}
      </div>
    </div>
  );
}
