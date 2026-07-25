import { invoke } from "@tauri-apps/api/core";

export type WorkspaceState = {
  openTabs: string[];
  activeTab: string | null;
  expandedDirs: string[];
  sidebarOpen: boolean;
  rightTool: string | null;
  bottomTool: string | null;
};

export const DEFAULT_WORKSPACE_STATE: WorkspaceState = {
  openTabs: [],
  activeTab: null,
  expandedDirs: ["."],
  sidebarOpen: true,
  rightTool: "assistant",
  bottomTool: null,
};

export function getWorkspaceState(projectRoot: string): Promise<WorkspaceState> {
  return invoke<WorkspaceState>("get_workspace_state", { projectRoot });
}

export function saveWorkspaceState(
  projectRoot: string,
  state: WorkspaceState,
): Promise<void> {
  return invoke<void>("save_workspace_state", { projectRoot, state });
}
