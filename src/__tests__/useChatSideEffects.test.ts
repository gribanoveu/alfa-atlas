import { act, renderHook } from "@testing-library/react";
import { describe, expect, test } from "bun:test";
import { useChatSideEffects } from "../hooks/useChatSideEffects";
import type { ToolResult } from "../lib/aiTools";
import type {
  ChatMessage,
  ToolCallBlock,
} from "../lib/chatBlocks";

function settled(
  id: string,
  name: string,
  result: ToolResult,
): ToolCallBlock {
  return {
    type: "toolCall",
    id,
    name,
    argumentsJson: "{}",
    status: "done",
    result,
  };
}

function assistant(...blocks: ToolCallBlock[]): ChatMessage[] {
  return [{ id: "assistant-1", role: "assistant", blocks }];
}

describe("useChatSideEffects", () => {
  test("reports settled file and access results once", () => {
    const writes: { tool: string; path: string }[] = [];
    const moves: { from: string; to: string }[] = [];
    let refreshes = 0;
    const callbacks = {
      onSendingChange: () => {},
      refreshAccessMode: async () => {
        refreshes += 1;
      },
      onConversationModeChange: () => {},
      onFileWritten: (info: { tool: string; path: string }) =>
        writes.push(info),
      onFileMoved: (info: { from: string; to: string }) =>
        moves.push(info),
    };
    const messages = assistant(
      settled("access", "requestFullRepoAccess", {
        tool: "accessModeChanged",
        result: { mode: "fullRepo" },
      }),
      settled("write", "writeFile", {
        tool: "fileWritten",
        result: {
          path: "guide.adoc",
          diff: {
            linesAdded: 1,
            linesRemoved: 0,
            unifiedDiff: "",
            truncated: false,
          },
        },
      }),
      settled("move", "move", {
        tool: "moved",
        result: {
          from: "old.adoc",
          to: "new.adoc",
          updatedFiles: [],
        },
      }),
    );

    const { rerender } = renderHook(
      ({ current }) =>
        useChatSideEffects({
          messages: current,
          initialMessages: [],
          sending: false,
          ...callbacks,
        }),
      { initialProps: { current: messages } },
    );

    expect(refreshes).toBe(1);
    expect(writes).toEqual([
      { tool: "writeFile", path: "guide.adoc" },
    ]);
    expect(moves).toEqual([
      { from: "old.adoc", to: "new.adoc", updatedFiles: [] },
    ]);
    rerender({ current: [...messages] });
    expect(refreshes).toBe(1);
    expect(writes).toHaveLength(1);
    expect(moves).toHaveLength(1);
  });

  test("defers a live mode switch until sending settles", () => {
    const modes: string[] = [];
    const messages = assistant(
      settled("mode", "requestModeSwitch", {
        tool: "modeSwitchRequested",
        result: { mode: "plan", reason: "Нужен план" },
      }),
    );
    const stable = {
      onSendingChange: () => {},
      refreshAccessMode: async () => {},
      onConversationModeChange: (mode: string) => modes.push(mode),
      onFileWritten: () => {},
      onFileMoved: () => {},
    };

    const { rerender } = renderHook(
      ({ sending }) =>
        useChatSideEffects({
          messages,
          initialMessages: [],
          sending,
          ...stable,
        }),
      { initialProps: { sending: true } },
    );

    expect(modes).toEqual([]);
    act(() => rerender({ sending: false }));
    expect(modes).toEqual(["plan"]);
    rerender({ sending: false });
    expect(modes).toEqual(["plan"]);
  });

  test("does not replay a historical mode switch on mount", () => {
    const modes: string[] = [];
    const messages = assistant(
      settled("old-mode", "requestModeSwitch", {
        tool: "modeSwitchRequested",
        result: { mode: "question", reason: "История" },
      }),
    );

    renderHook(() =>
      useChatSideEffects({
        messages,
        initialMessages: messages,
        sending: false,
        onSendingChange: () => {},
        refreshAccessMode: async () => {},
        onConversationModeChange: (mode) => modes.push(mode),
        onFileWritten: () => {},
        onFileMoved: () => {},
      }),
    );

    expect(modes).toEqual([]);
  });
});
