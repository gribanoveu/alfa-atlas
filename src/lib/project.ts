import { invoke } from "@tauri-apps/api/core";

export type OpenedProject = {
  root: string;
  docsRoot: string;
};

export type DocsCandidate = {
  path: string;
  relativePath: string;
  score: number;
  reason: string;
};

export type ProbeResult = {
  needsConfirm: boolean;
  root: string;
  docsRoot: string | null;
  candidates: DocsCandidate[];
  suggestedDocsRoot: string | null;
};

export type TreeNode = {
  name: string;
  path: string;
  isDir: boolean;
  children?: TreeNode[];
};

export type RecentProject = {
  root: string;
  name: string;
};

export type PathExistsResult = {
  exists: boolean;
  isDir: boolean;
  isNonEmpty: boolean;
};

export function probeOpenPath(path: string): Promise<ProbeResult> {
  return invoke<ProbeResult>("probe_open_path", { path });
}

export function openProject(
  root: string,
  docsRoot: string,
): Promise<OpenedProject> {
  return invoke<OpenedProject>("open_project", { root, docsRoot });
}

export function addGitignoreEntry(
  root: string,
  entry: string,
): Promise<void> {
  return invoke<void>("add_gitignore_entry", { root, entry });
}

/** Idempotent `.atlas/*` ignore + `!.atlas/memory/**` exception block. */
export function ensureAtlasGitignore(root: string): Promise<void> {
  return invoke<void>("ensure_atlas_gitignore", { root });
}

export function openCachedProject(root: string): Promise<OpenedProject> {
  return invoke<OpenedProject>("open_cached_project", { root });
}

export function getProject(): Promise<OpenedProject | null> {
  return invoke<OpenedProject | null>("get_project");
}

export function getSavedRepoRoot(): Promise<string | null> {
  return invoke<string | null>("get_saved_repo_root");
}

export function clearProject(): Promise<void> {
  return invoke<void>("clear_project");
}

export function listRecentProjects(): Promise<RecentProject[]> {
  return invoke<RecentProject[]>("list_recent_projects");
}

export function removeRecentProject(root: string): Promise<void> {
  return invoke<void>("remove_recent_project", { root });
}

export function getGitBranch(root: string): Promise<string | null> {
  return invoke<string | null>("get_git_branch", { root });
}

export function listDocsTree(docsRoot: string): Promise<TreeNode[]> {
  return invoke<TreeNode[]>("list_docs_tree", { docsRoot });
}

export function readProjectFile(
  docsRoot: string,
  relativePath: string,
): Promise<string> {
  return invoke<string>("read_project_file", { docsRoot, relativePath });
}

/** Same boundary as `readProjectFile`, but a missing file resolves to
 * `null` instead of a rejected promise — used by the assistant's `writeFile`
 * approval diff to distinguish "doesn't exist yet, show an empty original"
 * from a real failure. */
export function readProjectFileOrNone(
  docsRoot: string,
  relativePath: string,
): Promise<string | null> {
  return invoke<string | null>("read_project_file_or_none", { docsRoot, relativePath });
}

/**
 * Validate an asset path (e.g. image) against docsRoot on the backend and
 * return a canonical absolute filesystem path. The frontend turns it into
 * a WebView-loadable URL via `convertFileSrc` from `@tauri-apps/api/core`.
 */
export function resolveAssetPath(
  docsRoot: string,
  relativePath: string,
): Promise<string> {
  return invoke<string>("resolve_asset_path", { docsRoot, relativePath });
}

/** Docs-root-relative image file for `image::` completions. */
export type ImageFileEntry = {
  relativePath: string;
  fileName: string;
};

/** List image assets under docsRoot (gitignore-aware). */
export function listImageFiles(docsRoot: string): Promise<ImageFileEntry[]> {
  return invoke<ImageFileEntry[]>("list_image_files", { docsRoot });
}

/** Copy an OS file into docsRoot/destDirRelative/. Returns the new relative path. */
export function importExternalFile(
  docsRoot: string,
  destDirRelative: string,
  sourceAbsolute: string,
): Promise<string> {
  return invoke<string>("import_external_file", {
    docsRoot,
    destDirRelative,
    sourceAbsolute,
  });
}

/** Read a supported text file from an absolute path outside the project. */
export function readExternalTextFile(absolutePath: string): Promise<string> {
  return invoke<string>("read_external_text_file", { absolutePath });
}

/** Write a supported text file at an absolute path outside the project. */
export function writeExternalTextFile(
  absolutePath: string,
  content: string,
): Promise<void> {
  return invoke<void>("write_external_text_file", { absolutePath, content });
}

export function writeProjectFile(
  docsRoot: string,
  relativePath: string,
  content: string,
): Promise<void> {
  return invoke<void>("write_project_file", {
    docsRoot,
    relativePath,
    content,
  });
}

export function createProjectFile(
  docsRoot: string,
  relativePath: string,
): Promise<void> {
  return invoke<void>("create_project_file", { docsRoot, relativePath });
}

export type AsciidocFileTemplate = "method" | "request" | "response";

export function createProjectFileFromTemplate(
  docsRoot: string,
  relativePath: string,
  template: AsciidocFileTemplate | null,
): Promise<void> {
  return invoke<void>("create_project_file_from_template", {
    docsRoot,
    relativePath,
    template,
  });
}

export function createRestEndpointFolder(
  docsRoot: string,
  relativePath: string,
  methodName: string,
): Promise<void> {
  return invoke<void>("create_rest_endpoint_folder", {
    docsRoot,
    relativePath,
    methodName,
  });
}

export function createProjectDir(
  docsRoot: string,
  relativePath: string,
): Promise<void> {
  return invoke<void>("create_project_dir", { docsRoot, relativePath });
}

export function deleteProjectFile(
  docsRoot: string,
  relativePath: string,
): Promise<void> {
  return invoke<void>("delete_project_file", { docsRoot, relativePath });
}

export function deleteProjectDir(
  docsRoot: string,
  relativePath: string,
): Promise<void> {
  return invoke<void>("delete_project_dir", { docsRoot, relativePath });
}

/** One other document whose references were rewritten as a side effect of a rename/move. */
export type UpdatedReference = {
  docsRelativePath: string;
  count: number;
};

/** Result of a rename/move that cascaded into other documents' `include::`/`image::`/`xref:` references. */
export type RenameReport = {
  updatedFiles: UpdatedReference[];
};

export function renameProjectFile(
  docsRoot: string,
  fromPath: string,
  toPath: string,
): Promise<RenameReport> {
  return invoke<RenameReport>("rename_project_file", {
    docsRoot,
    fromRelative: fromPath,
    toRelative: toPath,
  });
}

export function renameProjectDir(
  docsRoot: string,
  fromPath: string,
  toPath: string,
): Promise<RenameReport> {
  return invoke<RenameReport>("rename_project_dir", {
    docsRoot,
    fromRelative: fromPath,
    toRelative: toPath,
  });
}

export function copyProjectFile(
  docsRoot: string,
  fromPath: string,
  toPath: string,
): Promise<void> {
  return invoke<void>("copy_project_file", {
    docsRoot,
    fromRelative: fromPath,
    toRelative: toPath,
  });
}

export function copyProjectDir(
  docsRoot: string,
  fromPath: string,
  toPath: string,
): Promise<void> {
  return invoke<void>("copy_project_dir", {
    docsRoot,
    fromRelative: fromPath,
    toRelative: toPath,
  });
}

export function checkPathExists(path: string): Promise<PathExistsResult> {
  return invoke<PathExistsResult>("check_path_exists", { path });
}
