import { ARTIFACT_KIND_LABELS, type ArtifactSummary } from "./artifacts";

/** Filtering and ordering for the artifacts list.
 *
 *  Pure, and separate from `useArtifacts` (which owns loading) because the
 *  list now spans every project: which rows a given filter should produce
 *  is a rule worth testing on its own, not something to read off the
 *  rendered table.
 *
 *  The key for a project is its **root path**, never its display name: two
 *  checkouts can end in the same folder name, and merging them under one
 *  filter entry would quietly hide half the artifacts. */

export type ArtifactSortKey = "recent" | "project" | "kind" | "title";

export type ArtifactFilters = {
  search: string;
  /** A project's root path, or `""` for «все проекты». */
  project: string;
  /** An `ArtifactKind`, or `""` for «все типы». */
  kind: string;
};

export const ARTIFACT_FILTERS_NONE: ArtifactFilters = { search: "", project: "", kind: "" };

export const ARTIFACT_SORT_OPTIONS: { value: ArtifactSortKey; label: string }[] = [
  { value: "recent", label: "Сначала новые" },
  { value: "project", label: "По проекту" },
  { value: "kind", label: "По типу" },
  { value: "title", label: "По названию" },
];

export const ARTIFACT_KIND_OPTIONS: { value: string; label: string }[] = [
  { value: "", label: "Все типы" },
  ...Object.entries(ARTIFACT_KIND_LABELS).map(([value, label]) => ({ value, label })),
];

/** Shown wherever a record predates roots being recorded. */
const UNKNOWN_PROJECT_LABEL = "Без проекта";

export function artifactProjectKey(artifact: ArtifactSummary): string {
  return artifact.repoRoot ?? "";
}

export function artifactProjectLabel(artifact: ArtifactSummary): string {
  return artifact.repoName || UNKNOWN_PROJECT_LABEL;
}

/** The last `segments` path segments of `root`, e.g. `WLBUH/enp-api`. */
function tail(root: string, segments: number): string {
  return root.split("/").filter(Boolean).slice(-segments).join("/");
}

/** «Все проекты» plus one entry per project present in `artifacts`,
 *  alphabetical.
 *
 *  Where two roots end in the same folder name — the same service checked
 *  out twice, which is normal here — the colliding entries grow by one
 *  parent directory at a time until they differ. Growing from the end
 *  rather than showing the whole path keeps the part that identifies the
 *  service visible: a filter 280px wide truncates `/Users/…/WORK_REPOS/…`
 *  long before it reaches the repository's name. */
export function artifactProjectOptions(
  artifacts: ArtifactSummary[],
): { value: string; label: string }[] {
  const byKey = new Map<string, string>();
  for (const artifact of artifacts) {
    const key = artifactProjectKey(artifact);
    if (!byKey.has(key)) byKey.set(key, artifactProjectLabel(artifact));
  }

  const roots = [...byKey.keys()].filter(Boolean);
  const deepest = Math.max(1, ...roots.map((root) => root.split("/").filter(Boolean).length));
  let segments = 1;
  while (segments < deepest) {
    const labels = roots.map((root) => tail(root, segments));
    if (new Set(labels).size === labels.length) break;
    segments += 1;
  }

  const options = [...byKey.entries()]
    .map(([value, label]) => ({ value, label: value ? tail(value, segments) : label }))
    .sort((a, b) => a.label.localeCompare(b.label, "ru"));

  return [{ value: "", label: "Все проекты" }, ...options];
}

function matchesSearch(artifact: ArtifactSummary, needle: string): boolean {
  if (!needle) return true;
  const haystack = [
    artifact.title,
    artifact.subtitle,
    artifact.repoName,
    ARTIFACT_KIND_LABELS[artifact.kind],
  ]
    .join(" ")
    .toLowerCase();
  return haystack.includes(needle);
}

const byRecency = (a: ArtifactSummary, b: ArtifactSummary) => b.updatedAtMs - a.updatedAtMs;

const KIND_ORDER = Object.keys(ARTIFACT_KIND_LABELS);

/** A kind with no declared position sorts last rather than first, which is
 *  what `indexOf`'s `-1` would otherwise mean. */
const kindOrder = (kind: ArtifactSummary["kind"]): number => {
  const index = KIND_ORDER.indexOf(kind);
  return index === -1 ? KIND_ORDER.length : index;
};

export function filterAndSortArtifacts(
  artifacts: ArtifactSummary[],
  filters: ArtifactFilters,
  sort: ArtifactSortKey,
): ArtifactSummary[] {
  const needle = filters.search.trim().toLowerCase();
  const matched = artifacts.filter(
    (artifact) =>
      (!filters.project || artifactProjectKey(artifact) === filters.project) &&
      (!filters.kind || artifact.kind === filters.kind) &&
      matchesSearch(artifact, needle),
  );

  // Every grouping order falls back to recency inside the group, so a
  // project's or a kind's own rows still read newest-first.
  const compare: Record<ArtifactSortKey, (a: ArtifactSummary, b: ArtifactSummary) => number> = {
    recent: byRecency,
    project: (a, b) =>
      artifactProjectLabel(a).localeCompare(artifactProjectLabel(b), "ru") || byRecency(a, b),
    // By the declared order of the kinds, not by their labels: the labels
    // mix Latin and Cyrillic («HTTP-запрос» / «Тикет Jira»), and how a
    // collator orders that pair turns out to depend on which ICU data the
    // runtime has loaded — a sort order must not vary between machines.
    kind: (a, b) => kindOrder(a.kind) - kindOrder(b.kind) || byRecency(a, b),
    title: (a, b) => a.title.localeCompare(b.title, "ru") || byRecency(a, b),
  };

  return matched.sort(compare[sort]);
}
