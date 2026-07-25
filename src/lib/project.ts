import { invoke } from "@tauri-apps/api/core";

export function getProjectRoot(): Promise<string | null> {
  return invoke<string | null>("get_project_root");
}

export function setProjectRoot(path: string): Promise<string> {
  return invoke<string>("set_project_root", { path });
}

export function clearProjectRoot(): Promise<void> {
  return invoke<void>("clear_project_root");
}
