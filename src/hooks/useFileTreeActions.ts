import { useCallback, useState } from "react";
import type { FileTreeDeleteTarget } from "../components/Sidebar/FileTree";
import { toMessage } from "../lib/errors";
import { importExternalFile } from "../lib/project";
import { isImageAsset, isSupportedFile } from "../lib/supportedFiles";
import type { useDocsTree } from "./useDocsTree";
import type { useEditorTabs } from "./useEditorTabs";
import type { useGitPanel } from "./useGitPanel";
import type { useProject } from "./useProject";
import type { useWorkspaceSession } from "./useWorkspaceSession";

type Deps = {
  project: ReturnType<typeof useProject>;
  tree: ReturnType<typeof useDocsTree>;
  session: ReturnType<typeof useWorkspaceSession>;
  editor: ReturnType<typeof useEditorTabs>;
  git: ReturnType<typeof useGitPanel>;
  showSuccess: (message: string) => void;
  setError: (message: string) => void;
};

/** The file tree's pending operations — which dialog is open and on what —
 * plus the two actions that touch more than the tree itself.
 *
 * `applyRenameReport` exists because a rename can rewrite `include::`,
 * `image::` and `xref:` in *other* documents. Those files may be open, so
 * their tabs are reloaded from disk (a no-op for anything not currently
 * open) and the user is told how much moved under them.
 *
 * `importExternal` is the OS drag-and-drop path: it keeps going after a
 * failed file rather than aborting the batch, since dropping ten files and
 * losing nine to one bad one would be worse than a single error message. */
export function useFileTreeActions({
  project,
  tree,
  session,
  editor,
  git,
  showSuccess,
  setError,
}: Deps) {
  const [newFileParent, setNewFileParent] = useState<string | null>(null);
  const [newFolderParent, setNewFolderParent] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<FileTreeDeleteTarget | null>(null);
  const [copiedItem, setCopiedItem] = useState<FileTreeDeleteTarget | null>(null);
  const [renameTarget, setRenameTarget] = useState<FileTreeDeleteTarget | null>(null);

  const applyRenameReport = useCallback(
    async (report: { updatedFiles: { docsRelativePath: string; count: number }[] }) => {
      if (report.updatedFiles.length === 0) return;
      await Promise.all(
        report.updatedFiles.map((f) => editor.reloadTabFromDisk(f.docsRelativePath)),
      );
      const totalRefs = report.updatedFiles.reduce((sum, f) => sum + f.count, 0);
      // Родительный падеж числительного не согласуем со словом — как в
      // «Найдено результатов: N» — чтобы не городить русскую плюрализацию
      // ради тоста.
      showSuccess(
        `Ссылки обновлены — файлов: ${report.updatedFiles.length}, ссылок: ${totalRefs}`,
      );
    },
    [editor.reloadTabFromDisk, showSuccess],
  );

  const importExternal = useCallback(
    async (destDirPath: string, absolutePaths: string[]) => {
      const docsRoot = project.docsRoot;
      if (!docsRoot) return;
      let lastOpened: string | null = null;
      for (const sourceAbsolute of absolutePaths) {
        try {
          const rel = await importExternalFile(docsRoot, destDirPath, sourceAbsolute);
          if (isSupportedFile(rel) || isImageAsset(rel)) {
            lastOpened = rel;
          }
        } catch (e) {
          setError(toMessage(e));
        }
      }
      session.ensureExpanded(destDirPath);
      await tree.refresh();
      git.scheduleRefresh();
      if (lastOpened) {
        void editor.openFile(lastOpened);
      }
    },
    [project.docsRoot, session, tree, git, editor, setError],
  );

  return {
    newFileParent,
    setNewFileParent,
    newFolderParent,
    setNewFolderParent,
    deleteTarget,
    setDeleteTarget,
    copiedItem,
    setCopiedItem,
    renameTarget,
    setRenameTarget,
    applyRenameReport,
    importExternal,
  };
}
