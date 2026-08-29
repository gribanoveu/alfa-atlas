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
  /** Palette rendered Mermaid diagrams use. */
  diagramTheme: DiagramTheme;
  /** Colour painted behind a rendered diagram: `"auto"` (follow
   *  `diagramTheme`), `"transparent"`, or a hex literal — see
   *  `normalizeDiagramBackdrop` and `resolveDiagramBackdrop`. */
  diagramBackdrop: string;
};

/** Mermaid and PlantUML draw dark-on-light, so a plate goes behind the SVG
 *  instead of letting the app's dark chrome show through. White is what it
 *  has always been; the pref makes it choosable. Mirrors
 *  `domain::settings::DEFAULT_DIAGRAM_BACKDROP`. */
export const DEFAULT_DIAGRAM_BACKDROP = "auto";

/** Which palette rendered diagrams use. Mirrors
 *  `domain::settings::DiagramTheme`. Only Mermaid honours it: PlantUML's
 *  `!theme` directives are a silent no-op in the bundled TeaVM engine, and
 *  the one thing that does work — injecting `skinparam` lines — would mean
 *  rewriting author-written diagram source. */
export type DiagramTheme = "dark" | "light";

export const DEFAULT_DIAGRAM_THEME: DiagramTheme = "dark";

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
  diagramTheme: DEFAULT_DIAGRAM_THEME,
  diagramBackdrop: DEFAULT_DIAGRAM_BACKDROP,
};

/** Presets offered in Настройки → Оформление, alongside the free picker. */
export const DIAGRAM_BACKDROP_PRESETS: { value: string; label: string }[] = [
  { value: "auto", label: "Авто" },
  { value: "#ffffff", label: "Белая" },
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
  const lower = trimmed.toLowerCase();
  if (lower === "auto") return "auto";
  if (lower === "transparent") return "transparent";
  return HEX_COLOR_RE.test(trimmed) ? trimmed.toLowerCase() : DEFAULT_DIAGRAM_BACKDROP;
}

/** The actual CSS colour to paint behind a diagram. `"auto"` is the point
 *  of this function: a dark-themed diagram wants the app's own chrome
 *  showing through, a light-themed one needs a light plate under it, and
 *  pairing those by hand is exactly the mistake the preset exists to
 *  prevent. Any explicit colour is passed through untouched. */
export function resolveDiagramBackdrop(
  backdrop: string,
  theme: DiagramTheme,
): string {
  const normalized = normalizeDiagramBackdrop(backdrop);
  if (normalized !== "auto") return normalized;
  return theme === "dark" ? "transparent" : "#ffffff";
}

export function normalizeDiagramTheme(value: unknown): DiagramTheme {
  return value === "light" ? "light" : DEFAULT_DIAGRAM_THEME;
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
    diagramTheme: normalizeDiagramTheme(prefs.diagramTheme),
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
