import { useEffect, useRef } from "react";
import type { ConversationMode } from "../lib/aiTools";
import type { ChatMessage } from "../lib/chatBlocks";
import type { UpdatedReference } from "../lib/project";

type ChatSideEffectsOptions = {
  messages: ChatMessage[];
  initialMessages: ChatMessage[];
  sending: boolean;
  onSendingChange: (sending: boolean) => void;
  refreshAccessMode: () => Promise<void>;
  onConversationModeChange: (mode: ConversationMode) => void;
  onFileWritten: (info: { tool: string; path: string }) => void;
  onFileMoved: (info: {
    from: string;
    to: string;
    updatedFiles: UpdatedReference[];
  }) => void;
};

function collectSettledModeSwitchIds(messages: ChatMessage[]): Set<string> {
  const ids = new Set<string>();
  for (const message of messages) {
    if (message.role !== "assistant") continue;
    for (const block of message.blocks) {
      if (
        block.type === "toolCall" &&
        block.name === "requestModeSwitch" &&
        block.status === "done"
      ) {
        ids.add(block.id);
      }
    }
  }
  return ids;
}

/**
 * Reacts to successful tool results after they have landed in the transcript.
 * Ref-backed handled sets keep the outward callbacks idempotent as settled
 * blocks remain in chat history forever.
 */
export function useChatSideEffects({
  messages,
  initialMessages,
  sending,
  onSendingChange,
  refreshAccessMode,
  onConversationModeChange,
  onFileWritten,
  onFileMoved,
}: ChatSideEffectsOptions): void {
  const handledAccessGrantIdsRef = useRef<Set<string>>(new Set());
  const handledModeSwitchIdsRef = useRef<Set<string>>(
    collectSettledModeSwitchIds(initialMessages),
  );
  const pendingModeSwitchRef = useRef<ConversationMode | null>(null);
  const handledFileWriteIdsRef = useRef<Set<string>>(new Set());
  const handledMoveIdsRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    onSendingChange(sending);
  }, [sending, onSendingChange]);

  useEffect(() => {
    const last = messages[messages.length - 1];
    if (!last || last.role !== "assistant") return;

    for (const block of last.blocks) {
      if (block.type !== "toolCall" || block.status !== "done") continue;

      if (
        block.name === "requestFullRepoAccess" &&
        !handledAccessGrantIdsRef.current.has(block.id)
      ) {
        handledAccessGrantIdsRef.current.add(block.id);
        void refreshAccessMode();
      }

      if (
        block.name === "requestModeSwitch" &&
        !handledModeSwitchIdsRef.current.has(block.id)
      ) {
        handledModeSwitchIdsRef.current.add(block.id);
        if (block.result?.tool === "modeSwitchRequested") {
          const nextMode = block.result.result.mode;
          if (sending) {
            pendingModeSwitchRef.current = nextMode;
          } else {
            pendingModeSwitchRef.current = null;
            onConversationModeChange(nextMode);
          }
        }
      }

      if (
        (block.name === "writeFile" ||
          block.name === "editFile" ||
          block.name === "deleteFile" ||
          block.name === "createDirectory" ||
          block.name === "deleteDirectory") &&
        !handledFileWriteIdsRef.current.has(block.id)
      ) {
        handledFileWriteIdsRef.current.add(block.id);
        const path =
          block.result &&
          (block.result.tool === "fileWritten" ||
            block.result.tool === "fileEdited" ||
            block.result.tool === "fileDeleted" ||
            block.result.tool === "directoryCreated" ||
            block.result.tool === "directoryDeleted")
            ? block.result.result.path
            : null;
        if (path !== null) onFileWritten({ tool: block.name, path });
      }

      if (block.name === "move" && !handledMoveIdsRef.current.has(block.id)) {
        handledMoveIdsRef.current.add(block.id);
        if (block.result?.tool === "moved") {
          const { from, to, updatedFiles } = block.result.result;
          onFileMoved({ from, to, updatedFiles });
        }
      }
    }
  }, [
    messages,
    sending,
    refreshAccessMode,
    onConversationModeChange,
    onFileWritten,
    onFileMoved,
  ]);

  useEffect(() => {
    if (sending) return;
    const pending = pendingModeSwitchRef.current;
    if (pending === null) return;
    pendingModeSwitchRef.current = null;
    onConversationModeChange(pending);
  }, [sending, onConversationModeChange]);
}
