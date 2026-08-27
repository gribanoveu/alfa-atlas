import {
  HTTP_STATUS_GROUPS,
  type HttpStatusCategory,
  type HttpStatusCode,
  type HttpStatusGroup,
} from "../data/httpStatusCodes";

export type HttpStatusFilter = HttpStatusCategory | "all";

export const HTTP_STATUS_FILTERS: { id: HttpStatusFilter; label: string }[] = [
  { id: "all", label: "Все" },
  { id: "1xx", label: "1xx" },
  { id: "2xx", label: "2xx" },
  { id: "3xx", label: "3xx" },
  { id: "4xx", label: "4xx" },
  { id: "5xx", label: "5xx" },
];

function matchesQuery(entry: HttpStatusCode, query: string): boolean {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return true;

  return (
    String(entry.code).includes(normalized) ||
    entry.name.toLowerCase().includes(normalized) ||
    entry.description.toLowerCase().includes(normalized) ||
    entry.usage.toLowerCase().includes(normalized)
  );
}

/** Фильтрует справочник по классу кодов и строке поиска. */
export function filterHttpStatusGroups(
  query: string,
  category: HttpStatusFilter = "all",
): HttpStatusGroup[] {
  return HTTP_STATUS_GROUPS.map((group) => ({
    ...group,
    codes: group.codes.filter(
      (entry) =>
        (category === "all" || entry.category === category) && matchesQuery(entry, query),
    ),
  })).filter((group) => group.codes.length > 0);
}

export function countHttpStatusMatches(
  query: string,
  category: HttpStatusFilter = "all",
): number {
  return filterHttpStatusGroups(query, category).reduce(
    (total, group) => total + group.codes.length,
    0,
  );
}
