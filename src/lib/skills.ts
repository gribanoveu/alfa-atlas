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

/** Read-only preview: the files of one skill, `SKILL.md` first. Works for
 * disabled and broken skills too, unlike the assistant's `skill` tool. */
export function skillsFiles(source: SkillSource, name: string): Promise<string[]> {
  return invoke<string[]>("skills_files", { source, name });
}

export function skillsReadFile(source: SkillSource, name: string, path: string): Promise<string> {
  return invoke<string>("skills_read_file", { source, name, path });
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

/** Splits a `SKILL.md` into its YAML frontmatter and the Markdown after it —
 * the same fences `domain::agent_skills::split_frontmatter` parses. Without
 * this the viewer would render `name:`/`description:` as a setext heading.
 * `frontmatter` is `null` when there is none; `body` is then the whole file. */
export function splitSkillFrontmatter(content: string): {
  frontmatter: string | null;
  body: string;
} {
  const text = content.replace(/^\uFEFF/, "");
  const match = /^---[ \t]*\r?\n([\s\S]*?)\r?\n---[ \t]*(?:\r?\n|$)/.exec(text);
  if (!match) return { frontmatter: null, body: content };
  return { frontmatter: match[1], body: text.slice(match[0].length) };
}
