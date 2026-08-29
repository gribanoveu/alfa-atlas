import { invoke } from "@tauri-apps/api/core";

export type PanelLayout = {
  sidebarWidth: number;
  rightWidth: number;
  bottomHeight: number;
  externalHeight: number;
};

export const DEFAULT_PANEL_LAYOUT: PanelLayout = {
  sidebarWidth: 220,
  rightWidth: 340,
  bottomHeight: 220,
  externalHeight: 160,
};

export const PANEL_LAYOUT_LIMITS = {
  sidebarWidth: { min: 160, max: 480 },
  rightWidth: { min: 400, max: 672 },
  bottomHeight: { min: 120, max: 480 },
  externalHeight: { min: 80, max: 400 },
} as const;

export function clampPanelLayout(layout: PanelLayout): PanelLayout {
  const clamp = (value: number, min: number, max: number) =>
    Math.min(max, Math.max(min, value));

  return {
    sidebarWidth: clamp(
      layout.sidebarWidth,
      PANEL_LAYOUT_LIMITS.sidebarWidth.min,
      PANEL_LAYOUT_LIMITS.sidebarWidth.max,
    ),
    rightWidth: clamp(
      layout.rightWidth,
      PANEL_LAYOUT_LIMITS.rightWidth.min,
      PANEL_LAYOUT_LIMITS.rightWidth.max,
    ),
    bottomHeight: clamp(
      layout.bottomHeight,
      PANEL_LAYOUT_LIMITS.bottomHeight.min,
      PANEL_LAYOUT_LIMITS.bottomHeight.max,
    ),
    externalHeight: clamp(
      layout.externalHeight ?? DEFAULT_PANEL_LAYOUT.externalHeight,
      PANEL_LAYOUT_LIMITS.externalHeight.min,
      PANEL_LAYOUT_LIMITS.externalHeight.max,
    ),
  };
}

export function getProjectLayout(projectRoot: string): Promise<PanelLayout> {
  return invoke<PanelLayout>("get_project_layout", { projectRoot }).then(
    (layout) =>
      clampPanelLayout({
        ...DEFAULT_PANEL_LAYOUT,
        ...layout,
      }),
  );
}

export function saveProjectLayout(
  projectRoot: string,
  layout: PanelLayout,
): Promise<void> {
  return invoke<void>("save_project_layout", {
    projectRoot,
    layout: clampPanelLayout(layout),
  });
}
