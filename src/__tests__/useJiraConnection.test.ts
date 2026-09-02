import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { JiraSettingsView, JiraUser } from "../lib/jira";
import * as actualJira from "../lib/jira";

let view: JiraSettingsView;
let hasToken: boolean;
let user: JiraUser | null;
let currentUserError: string | null;
let currentUserCalls = 0;

mock.module("../lib/jira", () => ({
  ...actualJira,
  getJiraSettings: async () => view,
  jiraHasToken: async () => hasToken,
  jiraCurrentUser: async () => {
    currentUserCalls += 1;
    if (currentUserError) throw currentUserError;
    return user!;
  },
}));

const { useJiraConnection } = await import("../hooks/useJiraConnection");

beforeEach(() => {
  view = {
    settings: { baseUrl: "https://jira.example.com", trustedCertPem: null },
    bundledBaseUrl: null,
    hasBundledCert: false,
  };
  hasToken = true;
  user = {
    displayName: "Иван Петров",
    emailAddress: "ivan@example.com",
    accountId: "ipetrov",
    active: true,
  };
  currentUserError = null;
  currentUserCalls = 0;
});

describe("useJiraConnection", () => {
  test("shows the account behind the token once the round trip succeeds", async () => {
    const { result } = renderHook(() => useJiraConnection());
    await waitFor(() => expect(result.current.state.kind).toBe("connected"));

    expect(result.current.state).toEqual({ kind: "connected", user: user! });
  });

  test("reports a missing instance without calling Jira", async () => {
    view = { settings: { baseUrl: "  ", trustedCertPem: null }, bundledBaseUrl: null, hasBundledCert: false };

    const { result } = renderHook(() => useJiraConnection());
    await waitFor(() => expect(result.current.state.kind).toBe("unconfigured"));

    expect(result.current.state).toEqual({ kind: "unconfigured", missing: "instance" });
    expect(currentUserCalls).toBe(0);
  });

  test("a build-supplied address counts as configured", async () => {
    view = {
      settings: { baseUrl: "", trustedCertPem: null },
      bundledBaseUrl: "https://jira.build.example",
      hasBundledCert: true,
    };

    const { result } = renderHook(() => useJiraConnection());

    // An empty settings field is not "unconfigured" when the build ships an
    // address — the check must still reach Jira.
    await waitFor(() => expect(result.current.state.kind).toBe("connected"));
    expect(currentUserCalls).toBe(1);
  });

  test("reports a missing token without calling Jira", async () => {
    hasToken = false;

    const { result } = renderHook(() => useJiraConnection());
    await waitFor(() => expect(result.current.state.kind).toBe("unconfigured"));

    expect(result.current.state).toEqual({ kind: "unconfigured", missing: "token" });
    expect(currentUserCalls).toBe(0);
  });

  test("surfaces a rejected token as an error state", async () => {
    currentUserError = "Jira отклонила токен";

    const { result } = renderHook(() => useJiraConnection());
    await waitFor(() => expect(result.current.state.kind).toBe("error"));

    expect(result.current.state).toEqual({
      kind: "error",
      message: "Jira отклонила токен",
    });
  });

  test("refresh re-checks against the current settings", async () => {
    hasToken = false;
    const { result } = renderHook(() => useJiraConnection());
    await waitFor(() => expect(result.current.state.kind).toBe("unconfigured"));

    hasToken = true;
    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.state.kind).toBe("connected");
    expect(currentUserCalls).toBe(1);
  });
});
