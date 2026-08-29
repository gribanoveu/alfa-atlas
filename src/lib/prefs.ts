import { invoke } from "@tauri-apps/api/core";

export type ErrorLanguage = "ru" | "en";

export type GeneralPrefs = {
  restoreLastProject: boolean;
  autosaveEnabled: boolean;
  saveOnTabSwitch: boolean;
  autosaveDelayMs: number;
  separateExternalFolder: boolean;
  openApiRefFallbackEnabled: boolean;
  errorLanguage: ErrorLanguage;
  uiFontSizePx: number;
  sidebarFontSizePx: number;
  editorFontSizePx: number;
  previewFontSizePx: number;
  assistantFontSizePx: number;
  lastCloneDir: string | null;
  notificationsAlertsExpanded: boolean;
  notificationsOnboardingExpanded: boolean;
  /** Colour painted behind a rendered Mermaid/PlantUML diagram. Hex literal
   *  or `"transparent"` — see `normalizeDiagramBackdrop`. */
  diagramBackdrop: string;
};

/** Mermaid and PlantUML draw dark-on-light, so a plate goes behind the SVG
 *  instead of letting the app's dark chrome show through. White is what it
 *  has always been; the pref makes it choosable. Mirrors
 *  `domain::settings::DEFAULT_DIAGRAM_BACKDROP`. */
export const DEFAULT_DIAGRAM_BACKDROP = "#ffffff";

export const DEFAULT_GENERAL_PREFS: GeneralPrefs = {
  restoreLastProject: true,
  autosaveEnabled: true,
  saveOnTabSwitch: true,
  autosaveDelayMs: 1000,
  separateExternalFolder: true,
  openApiRefFallbackEnabled: true,
  errorLanguage: "ru",
  uiFontSizePx: 12.5,
  sidebarFontSizePx: 12,
  editorFontSizePx: 13,
  previewFontSizePx: 14,
  assistantFontSizePx: 13,
  lastCloneDir: null,
  notificationsAlertsExpanded: true,
  notificationsOnboardingExpanded: true,
  diagramBackdrop: DEFAULT_DIAGRAM_BACKDROP,
};

/** Presets offered in Настройки → Оформление, alongside the free picker. */
export const DIAGRAM_BACKDROP_PRESETS: { value: string; label: string }[] = [
  { value: DEFAULT_DIAGRAM_BACKDROP, label: "Белая" },
  { value: "#f5f5f4", label: "Тёплая" },
  { value: "#1e1f22", label: "Тёмная" },
  { value: "transparent", label: "Прозрачная" },
];

const HEX_COLOR_RE = /^#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

/** Mirrors `domain::settings::normalize_diagram_backdrop`. Both halves
 *  validate because the value is written into a CSS custom property, which
 *  React does not escape — an arbitrary string there can close the
 *  declaration and inject rules. */
export function normalizeDiagramBackdrop(value: string | null | undefined): string {
  // Tolerates a missing value on purpose: `clampGeneralPrefs` also runs
  // over prefs loaded from an older `settings.json` that predates this
  // field, where it is genuinely absent.
  if (typeof value !== "string") return DEFAULT_DIAGRAM_BACKDROP;
  const trimmed = value.trim();
  if (trimmed.toLowerCase() === "transparent") return "transparent";
  return HEX_COLOR_RE.test(trimmed) ? trimmed.toLowerCase() : DEFAULT_DIAGRAM_BACKDROP;
}

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
    assistantFontSizePx: clampFontSizePx(prefs.assistantFontSizePx),
    diagramBackdrop: normalizeDiagramBackdrop(prefs.diagramBackdrop),
  };
}

export type SettingsPaths = {
  userSettingsDir: string;
  plansDir: string;
  artifactsDir: string;
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
