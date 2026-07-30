import { useCallback, useState, type RefObject } from "react";
import { ChevronDown, ChevronLeft, ChevronRight } from "lucide-react";
import type { Section } from "./types";
import { InlineHtml } from "./InlineHtml";
import { sectionDisplayTitle } from "./asciidocOutline";

export type AscTocPlacement = "left" | "right" | "top";

/**
 * Chevron points toward the edge the panel collapses into when expanded
 * (click to send it there), and away from it once collapsed (click to pull
 * it back out) — e.g. a left-docked TOC points left to collapse, right to
 * reopen. The "top" block collapses vertically instead, so it keeps the
 * up/down chevron.
 */
function tocToggleIcon(placement: AscTocPlacement, collapsed: boolean) {
  if (placement === "top") return collapsed ? ChevronRight : ChevronDown;
  const pointsLeft = placement === "left" ? !collapsed : collapsed;
  return pointsLeft ? ChevronLeft : ChevronRight;
}

type TocEntryProps = {
  section: Section;
  maxLevel: number;
  onNavigate: (id: string) => void;
};

function TocEntry({ section, maxLevel, onNavigate }: TocEntryProps) {
  const level = section.getLevel() ?? 1;
  const id = section.getId();
  const children = section.getSections() as unknown as Section[];
  const showChildren = children.length > 0 && level < maxLevel;

  return (
    <li>
      <a
        className="asc-toc-link"
        href={id ? `#${id}` : undefined}
        onClick={(event) => {
          event.preventDefault();
          if (id) onNavigate(id);
        }}
      >
        <InlineHtml html={sectionDisplayTitle(section)} />
      </a>
      {showChildren ? (
        <ul>
          {children.map((child, i) => (
            <TocEntry
              key={child.getId() ?? i}
              section={child}
              maxLevel={maxLevel}
              onNavigate={onNavigate}
            />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

/**
 * Оглавление документа (`:toc:`): дерево заголовков до `:toclevels:` (по
 * умолчанию 2), с номерами разделов, если включён `:sectnums:`. Клик по
 * пункту плавно скроллит к соответствующему заголовку внутри превью —
 * скролл ищется через `containerRef`, а не `document`, чтобы не задеть
 * одноимённые id в другом открытом превью.
 */
export function AscToc({
  sections,
  title,
  maxLevel,
  placement,
  containerRef,
}: {
  sections: Section[];
  title: string;
  maxLevel: number;
  placement: AscTocPlacement;
  containerRef: RefObject<HTMLElement | null>;
}) {
  // Starts collapsed: a sidebar TOC that pops open full-width for a frame
  // before the width-based auto-collapse (see AsciiDocPreview.tsx) can react
  // reads as a flicker every time a document opens. Collapsed-by-default
  // avoids that outright — the user opts in by clicking the toggle.
  const [collapsed, setCollapsed] = useState(true);

  const onNavigate = useCallback(
    (id: string) => {
      const root = containerRef.current;
      if (!root) return;
      const target = root.querySelector(`#${CSS.escape(id)}`);
      target?.scrollIntoView({ behavior: "smooth", block: "start" });
    },
    [containerRef],
  );

  if (sections.length === 0) return null;

  const toggleLabel = collapsed ? "Развернуть оглавление" : "Свернуть оглавление";
  const ToggleIcon = tocToggleIcon(placement, collapsed);

  return (
    <nav
      className={`asc-toc asc-toc-${placement}${collapsed ? " is-collapsed" : ""}`}
      aria-label={title}
    >
      <div className="asc-toc-header">
        <span className="asc-toc-title">{title}</span>
        <button
          type="button"
          className="asc-toc-toggle"
          onClick={() => setCollapsed((v) => !v)}
          aria-expanded={!collapsed}
          title={toggleLabel}
          aria-label={toggleLabel}
        >
          <ToggleIcon size={14} aria-hidden />
        </button>
      </div>
      {!collapsed ? (
        <ul>
          {sections.map((section, i) => (
            <TocEntry
              key={section.getId() ?? i}
              section={section}
              maxLevel={maxLevel}
              onNavigate={onNavigate}
            />
          ))}
        </ul>
      ) : null}
    </nav>
  );
}
