import { describe, expect, test } from "bun:test";
import {
  artifactProjectOptions,
  filterAndSortArtifacts,
  type ArtifactFilters,
} from "../lib/artifactFilters";
import type { ArtifactSummary } from "../lib/artifacts";

function artifact(over: Partial<ArtifactSummary> = {}): ArtifactSummary {
  return {
    id: "a1",
    kind: "jiraTicket",
    title: "Задача",
    status: "ready",
    subtitle: "Пользователь может выгрузить отчёт",
    createdAtMs: 0,
    updatedAtMs: 0,
    repoRoot: "/repos/enp-api",
    repoName: "enp-api",
    ...over,
  };
}

const ALL: ArtifactFilters = { search: "", project: "", kind: "" };
const ids = (list: ArtifactSummary[]) => list.map((a) => a.id);

describe("filterAndSortArtifacts", () => {
  test("returns everything, newest first, with no filters", () => {
    const list = [
      artifact({ id: "old", updatedAtMs: 100 }),
      artifact({ id: "new", updatedAtMs: 300 }),
    ];
    expect(ids(filterAndSortArtifacts(list, ALL, "recent"))).toEqual(["new", "old"]);
  });

  test("filters by project root, not by display name", () => {
    // Two checkouts ending in the same folder name are different projects;
    // filtering on the name would fold them into one.
    const list = [
      artifact({ id: "here", repoRoot: "/a/enp-api", repoName: "enp-api" }),
      artifact({ id: "there", repoRoot: "/b/enp-api", repoName: "enp-api" }),
    ];
    expect(ids(filterAndSortArtifacts(list, { ...ALL, project: "/a/enp-api" }, "recent"))).toEqual([
      "here",
    ]);
  });

  test("filters by kind", () => {
    const list = [
      artifact({ id: "ticket", kind: "jiraTicket" }),
      artifact({ id: "request", kind: "httpRequest" }),
    ];
    expect(ids(filterAndSortArtifacts(list, { ...ALL, kind: "httpRequest" }, "recent"))).toEqual([
      "request",
    ]);
  });

  test("search covers the title, the summary, the project and the kind label", () => {
    const list = [
      artifact({ id: "byTitle", title: "Выгрузка реестра", subtitle: "", repoName: "one" }),
      artifact({ id: "bySubtitle", title: "—", subtitle: "POST /v1/documents", repoName: "two" }),
      artifact({ id: "byRepo", title: "—", subtitle: "", repoName: "ausn-api" }),
    ];
    expect(ids(filterAndSortArtifacts(list, { ...ALL, search: "реестр" }, "recent"))).toEqual([
      "byTitle",
    ]);
    expect(ids(filterAndSortArtifacts(list, { ...ALL, search: "/v1/doc" }, "recent"))).toEqual([
      "bySubtitle",
    ]);
    expect(ids(filterAndSortArtifacts(list, { ...ALL, search: "ausn" }, "recent"))).toEqual([
      "byRepo",
    ]);
  });

  test("search ignores case and surrounding spaces", () => {
    const list = [artifact({ id: "one", title: "Выгрузка Реестра" })];
    expect(ids(filterAndSortArtifacts(list, { ...ALL, search: "  ВЫГРУЗКА " }, "recent"))).toEqual([
      "one",
    ]);
  });

  test("grouping orders keep recency inside the group", () => {
    const list = [
      artifact({ id: "b-old", repoName: "beta", repoRoot: "/r/beta", updatedAtMs: 100 }),
      artifact({ id: "a-new", repoName: "alpha", repoRoot: "/r/alpha", updatedAtMs: 200 }),
      artifact({ id: "b-new", repoName: "beta", repoRoot: "/r/beta", updatedAtMs: 300 }),
    ];
    expect(ids(filterAndSortArtifacts(list, ALL, "project"))).toEqual(["a-new", "b-new", "b-old"]);
  });

  test("sorts by kind and by title", () => {
    const list = [
      artifact({ id: "ticket", kind: "jiraTicket", title: "Яблоко" }),
      artifact({ id: "request", kind: "httpRequest", title: "Арбуз" }),
    ];
    // Kinds sort in their declared order (httpRequest, then jiraTicket) —
    // not by label, whose Latin/Cyrillic mix collates differently depending
    // on the runtime's ICU data.
    expect(ids(filterAndSortArtifacts(list, ALL, "kind"))).toEqual(["request", "ticket"]);
    expect(ids(filterAndSortArtifacts(list, ALL, "title"))).toEqual(["request", "ticket"]);
  });

  test("does not mutate the list it was given", () => {
    const list = [
      artifact({ id: "old", updatedAtMs: 100 }),
      artifact({ id: "new", updatedAtMs: 300 }),
    ];
    filterAndSortArtifacts(list, ALL, "recent");
    expect(ids(list)).toEqual(["old", "new"]);
  });
});

describe("artifactProjectOptions", () => {
  test("offers every project present, alphabetically, after «все проекты»", () => {
    const options = artifactProjectOptions([
      artifact({ repoRoot: "/r/beta", repoName: "beta" }),
      artifact({ repoRoot: "/r/alpha", repoName: "alpha" }),
      artifact({ repoRoot: "/r/beta", repoName: "beta" }),
    ]);
    expect(options).toEqual([
      { value: "", label: "Все проекты" },
      { value: "/r/alpha", label: "alpha" },
      { value: "/r/beta", label: "beta" },
    ]);
  });

  test("adds parent directories only until the colliding names differ", () => {
    const options = artifactProjectOptions([
      artifact({ repoRoot: "/Users/e/WORK/wlbuh/enp-api", repoName: "enp-api" }),
      artifact({ repoRoot: "/Users/e/WORK/wowtax/enp-api", repoName: "enp-api" }),
    ]);
    // One segment deeper is enough, so the whole `/Users/e/WORK` prefix —
    // which would have eaten the width and told the reader nothing — stays
    // out of the label.
    expect(options.map((o) => o.label)).toEqual([
      "Все проекты",
      "wlbuh/enp-api",
      "wowtax/enp-api",
    ]);
  });

  test("leaves distinct names alone even when their paths are deep", () => {
    const options = artifactProjectOptions([
      artifact({ repoRoot: "/Users/e/WORK/wlbuh/enp-api", repoName: "enp-api" }),
      artifact({ repoRoot: "/Users/e/WORK/wlbuh/ausn-api", repoName: "ausn-api" }),
    ]);
    expect(options.map((o) => o.label)).toEqual(["Все проекты", "ausn-api", "enp-api"]);
  });

  test("names records that never recorded a project", () => {
    const options = artifactProjectOptions([artifact({ repoRoot: null, repoName: "" })]);
    expect(options).toEqual([
      { value: "", label: "Все проекты" },
      { value: "", label: "Без проекта" },
    ]);
  });
});
