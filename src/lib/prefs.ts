import { invoke } from "@tauri-apps/api/core";

export type GeneralPrefs = {
  restoreLastProject: boolean;
};

export type SettingsPaths = {
  userSettingsDir: string;
  projectRoot: string | null;
  projectDocflowDir: string | null;
};

export function getGeneralPrefs(): Promise<GeneralPrefs> {
  return invoke<GeneralPrefs>("get_general_prefs");
}

export function setGeneralPrefs(prefs: GeneralPrefs): Promise<void> {
  return invoke<void>("set_general_prefs", { prefs });
}

export function getSettingsPaths(): Promise<SettingsPaths> {
  return invoke<SettingsPaths>("get_settings_paths");
}
