import { invoke } from "@tauri-apps/api/core";

/** The user layer. An empty field falls back to whatever the build ships in
 * `system_providers.yaml` (see `JiraSettingsView`). */
export type JiraSettings = {
  /** Instance root, e.g. `https://jira.example.com` — no `/rest/...` path. */
  baseUrl: string;
  /** The project new issues are created in, remembered between sessions.
   *  Empty until the user picks one. */
  projectKey: string;
  /** Display name for `projectKey` — a cache, never an identity: only the
   *  key is ever sent to Jira. */
  projectName: string;
  /** PEM bundle replacing the public trust roots for Jira requests. */
  trustedCertPem: string | null;
};

/** What the settings tab reads: the user's own values, plus what the build
 * would fall back to for each field they left empty. */
export type JiraSettingsView = {
  settings: JiraSettings;
  bundledBaseUrl: string | null;
  /** A flag rather than the PEM — the form only needs to say one is there. */
  hasBundledCert: boolean;
};

export type JiraUser = {
  displayName: string;
  emailAddress: string | null;
  accountId: string | null;
  active: boolean;
};

/** Whether a request is worth attempting at all. The token is not part of
 * settings, so `jiraHasToken` is the other half of the answer. */
export function isJiraAddressable(view: JiraSettingsView): boolean {
  return Boolean(view.settings.baseUrl.trim() || view.bundledBaseUrl);
}

export function getJiraSettings(): Promise<JiraSettingsView> {
  return invoke<JiraSettingsView>("jira_get_settings");
}

export function setJiraSettings(settings: JiraSettings): Promise<void> {
  return invoke<void>("jira_set_settings", { settings });
}

/** Write-only: the token never comes back over IPC. */
export function setJiraToken(token: string): Promise<void> {
  return invoke<void>("jira_set_token", { token });
}

export function jiraHasToken(): Promise<boolean> {
  return invoke<boolean>("jira_has_token");
}

export function deleteJiraToken(): Promise<void> {
  return invoke<void>("jira_delete_token");
}

/** The account behind the stored token — also the connection check, since
 * it only resolves when the whole chain (settings → token → TLS → HTTP)
 * worked. */
export function jiraCurrentUser(): Promise<JiraUser> {
  return invoke<JiraUser>("jira_current_user");
}

/** Mirrors `domain::jira::JiraWebLink` — one link to attach to an issue as
 *  a Jira Web Link (the issue's own Links panel), as opposed to the same URL
 *  sitting in the description text. Real tickets carry both. */
export type JiraWebLink = {
  url: string;
  /** Shown in the Links panel; falls back to the URL when empty. */
  title: string;
};

/** Mirrors `domain::jira::JiraLinkOutcome`. Reported per link because the
 *  interesting case is partial: some attach, one fails on a typo in the
 *  issue key. */
export type JiraLinkOutcome = {
  url: string;
  /** `null` on success; the reason otherwise. */
  error: string | null;
};

/** Attaches `links` to `issueKey`. Idempotent per link — Jira identifies a
 *  remote link by its URL and updates rather than duplicates — so a retry
 *  after a partial failure is safe. */
export function jiraAttachWebLinks(
  issueKey: string,
  links: JiraWebLink[],
): Promise<JiraLinkOutcome[]> {
  return invoke<JiraLinkOutcome[]>("jira_attach_web_links", { issueKey, links });
}

/** Mirrors `domain::jira::JiraProject`. `key` is the identity (it is what
 *  an issue key is built from); `name` is display only. */
export type JiraProject = {
  key: string;
  name: string;
  archived: boolean;
};

/** Projects the token can see. `recentOnly` is the default because the
 *  instance has thousands of them and the ten most recently used are what
 *  someone almost always wants; the full list is for searching. */
export function jiraListProjects(recentOnly: boolean): Promise<JiraProject[]> {
  return invoke<JiraProject[]>("jira_list_projects", { recentOnly });
}
