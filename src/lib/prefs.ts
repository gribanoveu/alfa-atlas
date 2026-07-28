import { invoke } from "@tauri-apps/api/core";

export type ErrorLanguage = "ru" | "en";

export type GeneralPrefs = {
  restoreLastProject: boolean;
  autosaveEnabled: boolean;
  saveOnTabSwitch: boolean;
  autosaveDelayMs: number;
  separateExternalFolder: boolean;
  errorLanguage: ErrorLanguage;
  uiFontSizePx: number;
  sidebarFontSizePx: number;
  editorFontSizePx: number;
  previewFontSizePx: number;
};

export const DEFAULT_GENERAL_PREFS: GeneralPrefs = {
  restoreLastProject: true,
  autosaveEnabled: true,
  saveOnTabSwitch: true,
  autosaveDelayMs: 1000,
  separateExternalFolder: true,
  errorLanguage: "ru",
  uiFontSizePx: 12.5,
  sidebarFontSizePx: 12,
  editorFontSizePx: 13,
  previewFontSizePx: 14,
};

export const AUTOSAVE_DELAY_LIMITS = { min: 300, max: 10_000 } as const;

export const FONT_SIZE_LIMITS = { min: 10, max: 24, step: 0.5 } as const;

export function clampAutosaveDelayMs(value: number): number {
  return Math.min(
    AUTOSAVE_DELAY_LIMITS.max,
    Math.max(AUTOSAVE_DELAY_LIMITS.min, Math.round(value)),
  );
}

export function clampFontSizePx(value: number): number {
  const clamped = Math.min(
    FONT_SIZE_LIMITS.max,
    Math.max(FONT_SIZE_LIMITS.min, value),
  );
  return Math.round(clamped * 2) / 2;
}

export function clampGeneralPrefs(prefs: GeneralPrefs): GeneralPrefs {
  return {
    ...prefs,
    autosaveDelayMs: clampAutosaveDelayMs(prefs.autosaveDelayMs),
    uiFontSizePx: clampFontSizePx(prefs.uiFontSizePx),
    sidebarFontSizePx: clampFontSizePx(prefs.sidebarFontSizePx),
    editorFontSizePx: clampFontSizePx(prefs.editorFontSizePx),
    previewFontSizePx: clampFontSizePx(prefs.previewFontSizePx),
  };
}

export type SettingsPaths = {
  userSettingsDir: string;
  projectRoot: string | null;
  projectConfigDir: string | null;
};

export function getGeneralPrefs(): Promise<GeneralPrefs> {
  return invoke<GeneralPrefs>("get_general_prefs");
}

export function setGeneralPrefs(prefs: GeneralPrefs): Promise<void> {
  return invoke<void>("set_general_prefs", {
    prefs: clampGeneralPrefs(prefs),
  });
}

export function getSettingsPaths(): Promise<SettingsPaths> {
  return invoke<SettingsPaths>("get_settings_paths");
}
