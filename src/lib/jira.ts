import { invoke } from "@tauri-apps/api/core";

/** The user layer. An empty field falls back to whatever the build ships in
 * `system_providers.yaml` (see `JiraSettingsView`). */
export type JiraSettings = {
  /** Instance root, e.g. `https://jira.example.com` — no `/rest/...` path. */
  baseUrl: string;
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
