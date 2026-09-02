import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { JiraSettings, JiraSettingsView } from "../lib/jira";
import * as actualJira from "../lib/jira";

function view(settings: Partial<JiraSettings> = {}): JiraSettingsView {
  return {
    settings: {
      baseUrl: "https://jira.example.com",
      projectKey: "",
      projectName: "",
      issueTypeId: "",
      issueTypeName: "",
      trustedCertPem: null,
      ...settings,
    },
    bundledBaseUrl: null,
    hasBundledCert: false,
  };
}

let stored: JiraSettingsView;
let writes: JiraSettings[] = [];
let failWith: string | null = null;

// Spreading the real module matters: `mock.module` is global in bun.
mock.module("../lib/jira", () => ({
  ...actualJira,
  getJiraSettings: async () => stored,
  setJiraSettings: async (next: JiraSettings) => {
    if (failWith) throw failWith;
    writes.push(next);
    // Mirrors the backend rule: an issue type belongs to its project.
    const projectChanged = next.projectKey !== stored.settings.projectKey;
    const keptOldType = next.issueTypeId === stored.settings.issueTypeId;
    stored = view({
      ...next,
      ...(projectChanged && keptOldType ? { issueTypeId: "", issueTypeName: "" } : {}),
    });
  },
}));

const { useJiraProject } = await import("../hooks/useJiraProject");

afterEach(() => {
  failWith = null;
});
beforeEach(() => {
  stored = view({ projectKey: "ALPHA", projectName: "Alpha", issueTypeId: "20", issueTypeName: "User Story" });
  writes = [];
});

describe("useJiraProject", () => {
  test("loads the remembered project and type", async () => {
    const { result } = renderHook(() => useJiraProject());
    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.projectKey).toBe("ALPHA");
    expect(result.current.issueTypeId).toBe("20");
  });

  // The backend clears the type when the project changes; showing the old
  // one afterwards would claim something that is no longer stored.
  test("switching the project drops the issue type from the view too", async () => {
    const { result } = renderHook(() => useJiraProject());
    await waitFor(() => expect(result.current.ready).toBe(true));

    await act(async () => {
      await result.current.pickProject({ key: "BETA", name: "Beta", archived: false });
    });

    expect(result.current.projectKey).toBe("BETA");
    expect(result.current.issueTypeId).toBe("");
  });

  test("picking a type keeps the project", async () => {
    const { result } = renderHook(() => useJiraProject());
    await waitFor(() => expect(result.current.ready).toBe(true));

    await act(async () => {
      await result.current.pickIssueType({ id: "3", name: "Task", subtask: false });
    });

    expect(result.current.projectKey).toBe("ALPHA");
    expect(result.current.issueTypeId).toBe("3");
  });

  test("a failed write rolls back to what is actually stored", async () => {
    const { result } = renderHook(() => useJiraProject());
    await waitFor(() => expect(result.current.ready).toBe(true));

    failWith = "нет доступа";
    await act(async () => {
      await result.current.pickProject({ key: "BETA", name: "Beta", archived: false });
    });

    expect(result.current.projectKey).toBe("ALPHA");
    expect(result.current.error).toBe("нет доступа");
    expect(writes).toEqual([]);
  });
});
