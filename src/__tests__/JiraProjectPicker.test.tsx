import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { JiraProject } from "../lib/jira";
import * as actualJira from "../lib/jira";

let recentCalls = 0;
let allCalls = 0;

const RECENT: JiraProject[] = [
  { key: "WOWTAX", name: "Бухгалтерия", archived: false },
  { key: "NIBDOC", name: "Документооборот", archived: false },
];

const ALL: JiraProject[] = [
  ...RECENT,
  { key: "APF", name: "AlfaPayFeeDiscount", archived: false },
  // Name-only match, to check it sorts after a key match.
  { key: "ZZZ", name: "Отчёты WOW", archived: false },
];

// Spreading the real module matters: `mock.module` is global in bun, so a
// replacement that dropped the other exports would break every other suite
// that imports from here.
mock.module("../lib/jira", () => ({
  ...actualJira,
  jiraListProjects: async (recentOnly: boolean) => {
    if (recentOnly) {
      recentCalls += 1;
      return RECENT;
    }
    allCalls += 1;
    return ALL;
  },
}));

const { JiraProjectPicker } = await import("../components/Jira/JiraProjectPicker");

afterEach(cleanup);
beforeEach(() => {
  recentCalls = 0;
  allCalls = 0;
});

function renderPicker(projectKey = "", projectName = "") {
  const picked: JiraProject[] = [];
  render(
    <JiraProjectPicker
      projectKey={projectKey}
      projectName={projectName}
      disabled={false}
      onPick={(p) => picked.push(p)}
    />,
  );
  return picked;
}

describe("JiraProjectPicker", () => {
  test("shows the remembered project without opening anything", () => {
    renderPicker("WOWTAX", "Бухгалтерия");
    expect(screen.getByText("WOWTAX")).toBeDefined();
    expect(recentCalls).toBe(0);
  });

  // The instance has thousands of projects; the ones someone last worked in
  // are the answer almost every time, so that is what opening shows.
  test("opens on the recent projects, not the full list", async () => {
    renderPicker();
    fireEvent.click(screen.getByText("Выбрать"));

    await waitFor(() => expect(screen.getByText("Бухгалтерия")).toBeDefined());
    expect(recentCalls).toBe(1);
    expect(allCalls).toBe(0);
  });

  test("the full list is fetched once, on the first search", async () => {
    renderPicker();
    fireEvent.click(screen.getByText("Выбрать"));
    await waitFor(() => expect(recentCalls).toBe(1));

    const search = screen.getByLabelText("Поиск проекта");
    fireEvent.change(search, { target: { value: "wow" } });
    await waitFor(() => expect(allCalls).toBe(1));

    fireEvent.change(search, { target: { value: "wowt" } });
    await waitFor(() => expect(screen.getByText("Бухгалтерия")).toBeDefined());
    // A keystroke must not be a request.
    expect(allCalls).toBe(1);
  });

  test("a key match sorts above a name match", async () => {
    renderPicker();
    fireEvent.click(screen.getByText("Выбрать"));
    await waitFor(() => expect(recentCalls).toBe(1));

    fireEvent.change(screen.getByLabelText("Поиск проекта"), { target: { value: "wow" } });
    await waitFor(() => expect(allCalls).toBe(1));

    const keys = screen
      .getAllByRole("listitem")
      .map((li) => li.textContent ?? "");
    expect(keys[0]).toContain("WOWTAX");
    expect(keys.some((k) => k.includes("ZZZ"))).toBe(true);
    expect(keys.indexOf(keys.find((k) => k.includes("ZZZ"))!)).toBeGreaterThan(0);
  });

  test("picking one reports it and closes the picker", async () => {
    const picked = renderPicker();
    fireEvent.click(screen.getByText("Выбрать"));
    await waitFor(() => expect(screen.getByText("Бухгалтерия")).toBeDefined());

    fireEvent.click(screen.getByText("Бухгалтерия"));

    expect(picked).toEqual([{ key: "WOWTAX", name: "Бухгалтерия", archived: false }]);
    expect(screen.queryByLabelText("Поиск проекта")).toBeNull();
  });
});
