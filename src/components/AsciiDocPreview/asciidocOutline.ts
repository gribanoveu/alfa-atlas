import type { Document, Section } from "./types";

/**
 * The number of leading section levels asciidoctor prefixes with a number
 * when `:sectnums:` is set — `[cols=...]`'s sibling attribute, `:sectnumlevels:`
 * (default 3). Deeper sections stay numbered internally (`sectnum()` still
 * resolves) but aren't displayed, matching the reference HTML5 converter.
 *
 * `getDocument()`'s return type is untyped in asciidoctor's vendored .d.ts
 * (no `Document` import there), so TS falls back to the ambient DOM
 * `Document` — hence the cast back to the real asciidoctor `Document`.
 */
export function sectnumlevelsOf(section: Section): number {
  const doc = section.getDocument() as unknown as Document;
  const value = doc.getAttribute("sectnumlevels", 3);
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 3;
}

/** Section title HTML, prefixed with its resolved number (e.g. "1.2.") when numbered. */
export function sectionDisplayTitle(section: Section): string | null {
  const title = section.title;
  if (!title) return title ?? null;
  const level = section.getLevel() ?? 1;
  if (section.isNumbered() && level <= sectnumlevelsOf(section)) {
    return `${section.sectnum()} ${title}`;
  }
  return title;
}
