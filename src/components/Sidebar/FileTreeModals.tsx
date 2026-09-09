import { DeleteConfirmModal } from "./DeleteConfirmModal";
import { NewFileModal } from "./NewFileModal";
import { NewFolderModal } from "./NewFolderModal";
import { RenameModal } from "./RenameModal";
import {
  createProjectDir,
  createProjectFileFromTemplate,
  createRestEndpointFolder,
  deleteProjectDir,
  deleteProjectFile,
  renameProjectDir,
  renameProjectFile,
} from "../../lib/project";
import { joinParent, parentOfPath } from "../../lib/paths";
import type { useDocsTree } from "../../hooks/useDocsTree";
import type { useEditorTabs } from "../../hooks/useEditorTabs";
import type { useFileTreeActions } from "../../hooks/useFileTreeActions";
import type { useWorkspaceSession } from "../../hooks/useWorkspaceSession";

type FileTreeModalsProps = {
  docsRoot: string | null;
  fileTree: ReturnType<typeof useFileTreeActions>;
  tree: ReturnType<typeof useDocsTree>;
  session: ReturnType<typeof useWorkspaceSession>;
  editor: ReturnType<typeof useEditorTabs>;
  onGitRefresh: () => void;
};

/** Create / rename / delete dialogs for the document tree.
 *
 * Lifted out of `App` unchanged. They are the only reader of four of
 * `useFileTreeActions`' state pairs, and their `onConfirm` handlers are the
 * bulk of what is left of App's render. */
export function FileTreeModals({
  docsRoot,
  fileTree,
  tree,
  session,
  editor,
  onGitRefresh,
}: FileTreeModalsProps) {
  const {
    newFileParent,
    setNewFileParent,
    newFolderParent,
    setNewFolderParent,
    deleteTarget,
    setDeleteTarget,
    renameTarget,
    setRenameTarget,
    applyRenameReport,
  } = fileTree;

  return (
    <>
  {newFileParent !== null && docsRoot ? (
    <NewFileModal
      parentPath={newFileParent}
      onCancel={() => setNewFileParent(null)}
      onConfirm={async (fileName, template) => {
        const relativePath = joinParent(newFileParent, fileName);
        await createProjectFileFromTemplate(
          docsRoot!,
          relativePath,
          template,
        );
        session.ensureExpanded(newFileParent);
        setNewFileParent(null);
        await tree.refresh();
        await editor.openFile(relativePath);
      }}
    />
  ) : null}

  {newFolderParent !== null && docsRoot ? (
    <NewFolderModal
      parentPath={newFolderParent}
      onCancel={() => setNewFolderParent(null)}
      onConfirm={async (folderName, useRestEndpointTemplate) => {
        const relativePath = joinParent(newFolderParent, folderName);
        if (useRestEndpointTemplate) {
          await createRestEndpointFolder(
            docsRoot!,
            relativePath,
            folderName,
          );
        } else {
          await createProjectDir(docsRoot!, relativePath);
        }
        session.ensureExpanded(relativePath);
        setNewFolderParent(null);
        await tree.refresh();
        if (useRestEndpointTemplate) {
          await editor.openFile(joinParent(relativePath, `${folderName}.adoc`));
        }
      }}
    />
  ) : null}

  {deleteTarget !== null && docsRoot ? (
    <DeleteConfirmModal
      target={deleteTarget}
      onCancel={() => setDeleteTarget(null)}
      onConfirm={async (target) => {
        if (target.isDir) {
          await deleteProjectDir(docsRoot!, target.path);
        } else {
          await deleteProjectFile(docsRoot!, target.path);
        }
        editor.discardTabsUnder(target.path);
        setDeleteTarget(null);
        await tree.refresh();
        onGitRefresh();
      }}
    />
  ) : null}

  {renameTarget !== null && docsRoot ? (
    <RenameModal
      target={renameTarget}
      onCancel={() => setRenameTarget(null)}
      onConfirm={async (newName) => {
        const oldPath = renameTarget.path;
        const newPath = joinParent(parentOfPath(oldPath), newName);
        const report = renameTarget.isDir
          ? await renameProjectDir(docsRoot!, oldPath, newPath)
          : await renameProjectFile(docsRoot!, oldPath, newPath);
        editor.remapTabsUnder(oldPath, newPath);
        session.remapExpandedUnder(oldPath, newPath);
        session.ensureExpanded(parentOfPath(newPath));
        setRenameTarget(null);
        await tree.refresh();
        onGitRefresh();
        void applyRenameReport(report);
      }}
    />
  ) : null}
    </>
  );
}
