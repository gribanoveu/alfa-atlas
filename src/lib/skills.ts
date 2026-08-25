import { invoke } from "@tauri-apps/api/core";

export type SkillSource = "bundled" | "user";

export type SkillListItem = {
  name: string;
  description: string;
  source: SkillSource;
  enabled: boolean;
  error: string | null;
};

export type SkillMeta = {
  name: string;
  description: string;
  source: SkillSource;
};

export function skillsList(): Promise<SkillListItem[]> {
  return invoke<SkillListItem[]>("skills_list");
}

export function skillsSetEnabled(source: SkillSource, name: string, enabled: boolean): Promise<void> {
  return invoke("skills_set_enabled", { source, name, enabled });
}

export function skillsImport(path: string): Promise<SkillMeta> {
  return invoke<SkillMeta>("skills_import", { path });
}

export function skillsRemove(name: string): Promise<void> {
  return invoke("skills_remove", { name });
}

export function skillsUserDir(): Promise<string> {
  return invoke<string>("skills_user_dir");
}
