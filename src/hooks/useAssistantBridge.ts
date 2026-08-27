import { useCallback, useRef, useState } from "react";
import type { ConversationMode } from "../lib/aiTools";
import { dirnameOf, toDocsRelativePath } from "../lib/paths";
import type { UpdatedReference } from "../lib/project";
import type { useDocsTree } from "./useDocsTree";
import type { useEditorTabs } from "./useEditorTabs";
import type { useGitPanel } from "./useGitPanel";
import type { useOpenApiBundle } from "./useOpenApiBundle";
import type { useProject } from "./useProject";
import type { useWorkspaceLayout } from "./useWorkspaceLayout";
import type { useWorkspaceSession } from "./useWorkspaceSession";

type Deps = {
  project: ReturnType<typeof useProject>;
  editor: ReturnType<typeof useEditorTabs>;
  tree: ReturnType<typeof useDocsTree>;
  session: ReturnType<typeof useWorkspaceSession>;
  git: ReturnType<typeof useGitPanel>;
  layout: ReturnType<typeof useWorkspaceLayout>;
  openApiBundle: ReturnType<typeof useOpenApiBundle>;
  /** Reloads open tabs whose references a rename rewrote, and reports how
   * much moved. Shared with the manual rename path. */
  applyRenameReport: (report: {
    updatedFiles: { docsRelativePath: string; count: number }[];
  }) => Promise<void>;
};

/** Everything that flows between the assistant panel and the rest of the app.
 *
 * Two directions. The assistant reports what it changed on disk, and the
 * editor/tree/git have to catch up — the assistant writes files directly, so
 * nothing else would notice. And the editor sends text the other way: a
 * snippet into the document, or a selection into the chat draft.
 *
 * Both text paths carry an incrementing id rather than the text alone.
 * Inserting the same snippet twice in a row must insert twice, and a prop
 * that compares equal would be dropped the second time. */
export function useAssistantBridge({
  project,
  editor,
  tree,
  session,
  git,
  layout,
  openApiBundle,
  applyRenameReport,
}: Deps) {
  const [insertRequest, setInsertRequest] = useState<{
    id: number;
    tabId: string;
    text: string;
  } | null>(null);
  const [chatInsertRequest, setChatInsertRequest] = useState<{
    id: number;
    text: string;
    filePath: string | null;
  } | null>(null);
  const [assistantSendRequest, setAssistantSendRequest] = useState<{
    id: number;
    text: string;
    conversationMode?: ConversationMode;
  } | null>(null);
  const [assistantDraftRequest, setAssistantDraftRequest] = useState<{
    id: number;
    text: string;
    conversationMode?: ConversationMode;
  } | null>(null);
  const insertCounter = useRef(0);
  const chatInsertCounter = useRef(0);
  const assistantSendCounter = useRef(0);
  const assistantDraftCounter = useRef(0);

  /** Tool results use access-mode-relative paths; editor tabs are
   * docs-relative. */
  const toDocs = useCallback(
    (p: string) =>
      project.repoRoot && project.docsRoot
        ? toDocsRelativePath(p, project.repoRoot, project.docsRoot)
        : p,
    [project.repoRoot, project.docsRoot],
  );

  const onFileWritten = useCallback(
    ({ tool, path }: { tool: string; path: string }) => {
      void tree.refresh();
      if (openApiBundle.bundle) void openApiBundle.reload();
      const docsPath = toDocs(path);
      switch (tool) {
        case "writeFile":
        case "editFile":
          void editor.reloadTabFromDisk(docsPath);
          break;
        case "deleteFile":
        case "deleteDirectory":
          editor.discardTabsUnder(docsPath);
          break;
        default:
          break;
      }
    },
    [tree, editor, openApiBundle, toDocs],
  );

  /** Same idea as `onFileWritten`, but a `move` has both an old and a new
   * path, so a reload is not enough — an open tab under `from` has to keep
   * pointing at the same file at its new path, exactly like the manual
   * drag-and-drop path does. The move's reference-rewrite report goes
   * through `applyRenameReport` so files that included or referenced the
   * moved one are reloaded and reported the same way a manual rename does. */
  const onFileMoved = useCallback(
    ({ from, to, updatedFiles }: { from: string; to: string; updatedFiles: UpdatedReference[] }) => {
      const docsFrom = toDocs(from);
      const docsTo = toDocs(to);
      editor.remapTabsUnder(docsFrom, docsTo);
      session.remapExpandedUnder(docsFrom, docsTo);
      session.ensureExpanded(dirnameOf(docsTo));
      void tree.refresh();
      if (openApiBundle.bundle) void openApiBundle.reload();
      git.scheduleRefresh();
      void applyRenameReport({
        updatedFiles: updatedFiles.map((f) => ({
          docsRelativePath: toDocs(f.docsRelativePath),
          count: f.count,
        })),
      });
    },
    [editor, session, tree, git, applyRenameReport, openApiBundle, toDocs],
  );

  const insertSnippet = useCallback(
    (text: string) => {
      const tabId = editor.activeTabId;
      if (!tabId) return;
      insertCounter.current += 1;
      setInsertRequest({ id: insertCounter.current, tabId, text });
    },
    [editor.activeTabId],
  );

  /** Opens the assistant dock if it is closed and drops the selection into
   * the chat draft. */
  const addSelectionToChat = useCallback(
    (text: string, filePath: string | null) => {
      chatInsertCounter.current += 1;
      layout.setRightTool("assistant");
      setChatInsertRequest({ id: chatInsertCounter.current, text, filePath });
    },
    [layout],
  );

  /** Cleared here rather than by a flag inside `AssistantConversation`,
   * which remounts on a chat switch or a dock tool change and would lose
   * such a flag — re-inserting the same request into the new draft. */
  const onChatInsertHandled = useCallback(() => {
    setChatInsertRequest(null);
  }, []);

  /** Opens the assistant dock and sends a canned prompt immediately (not a
   * draft). Used by editor context actions. */
  const sendAssistantPrompt = useCallback(
    (text: string, opts?: { conversationMode?: ConversationMode }) => {
      assistantSendCounter.current += 1;
      layout.setRightTool("assistant");
      setAssistantSendRequest({
        id: assistantSendCounter.current,
        text,
        conversationMode: opts?.conversationMode,
      });
    },
    [layout],
  );

  const onAssistantSendHandled = useCallback(() => {
    setAssistantSendRequest(null);
  }, []);

  /** Opens the assistant dock and fills the composer with a canned prompt. */
  const insertAssistantDraft = useCallback(
    (text: string, opts?: { conversationMode?: ConversationMode }) => {
      assistantDraftCounter.current += 1;
      layout.setRightTool("assistant");
      setAssistantDraftRequest({
        id: assistantDraftCounter.current,
        text,
        conversationMode: opts?.conversationMode,
      });
    },
    [layout],
  );

  const onAssistantDraftHandled = useCallback(() => {
    setAssistantDraftRequest(null);
  }, []);

  return {
    insertRequest,
    chatInsertRequest,
    assistantSendRequest,
    assistantDraftRequest,
    onFileWritten,
    onFileMoved,
    insertSnippet,
    addSelectionToChat,
    onChatInsertHandled,
    sendAssistantPrompt,
    onAssistantSendHandled,
    insertAssistantDraft,
    onAssistantDraftHandled,
  };
}
