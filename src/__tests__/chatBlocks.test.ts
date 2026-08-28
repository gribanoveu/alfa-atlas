import { describe, expect, test } from "bun:test";
import {
  appendDeltaToBlocks,
  appendReasoningDeltaToBlocks,
  appendToolCallBlock,
  chatMessageToPlainText,
  correctTrailingReasoning,
  correctTrailingText,
  flattenBlocksToText,
  groupBlocksForRender,
  markRunningToolCallsAsInterrupted,
  settleToolCallBlock,
  type ChatMessage,
  type MessageBlock,
  type ToolCallBlock,
} from "../lib/chatBlocks";

describe("appendDeltaToBlocks", () => {
  test("opens a new text block when there are no blocks yet", () => {
    const blocks = appendDeltaToBlocks([], "Hello");
    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toMatchObject({ type: "text", content: "Hello" });
  });

  test("extends the trailing text block", () => {
    const first = appendDeltaToBlocks([], "Hel");
    const second = appendDeltaToBlocks(first, "lo");
    expect(second).toHaveLength(1);
    expect(second[0]).toMatchObject({ type: "text", content: "Hello" });
  });

  test("opens a fresh text block after a trailing tool-call block", () => {
    const toolCall: ToolCallBlock = {
      type: "toolCall",
      id: "call_1",
      name: "readFile",
      argumentsJson: "{}",
      status: "running",
    };
    const blocks = appendDeltaToBlocks([toolCall], "After the tool call");
    expect(blocks).toHaveLength(2);
    expect(blocks[0]).toBe(toolCall);
    expect(blocks[1]).toMatchObject({ type: "text", content: "After the tool call" });
  });
});

describe("appendReasoningDeltaToBlocks", () => {
  test("opens a new reasoning block when there are no blocks yet", () => {
    const blocks = appendReasoningDeltaToBlocks([], "Let me think");
    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toMatchObject({ type: "reasoning", content: "Let me think" });
  });

  test("extends the trailing reasoning block", () => {
    const first = appendReasoningDeltaToBlocks([], "Let me ");
    const second = appendReasoningDeltaToBlocks(first, "think");
    expect(second).toHaveLength(1);
    expect(second[0]).toMatchObject({ type: "reasoning", content: "Let me think" });
  });

  test("a subsequent text delta closes the reasoning block off and opens a new text block", () => {
    const reasoning = appendReasoningDeltaToBlocks([], "thinking...");
    const withText = appendDeltaToBlocks(reasoning, "the answer");
    expect(withText).toHaveLength(2);
    expect(withText[0]).toMatchObject({ type: "reasoning", content: "thinking..." });
    expect(withText[1]).toMatchObject({ type: "text", content: "the answer" });
  });
});

describe("appendToolCallBlock", () => {
  test("always appends a new running block, regardless of trailing block type", () => {
    const withText = appendToolCallBlock(
      [{ type: "text", id: "t1", content: "thinking..." }],
      { id: "call_1", name: "listFiles", argumentsJson: "{}" },
    );
    expect(withText).toHaveLength(2);
    expect(withText[1]).toEqual({
      type: "toolCall",
      id: "call_1",
      name: "listFiles",
      argumentsJson: "{}",
      status: "running",
    });

    const fromEmpty = appendToolCallBlock([], { id: "call_2", name: "readFile", argumentsJson: "{}" });
    expect(fromEmpty).toHaveLength(1);
    expect(fromEmpty[0].type).toBe("toolCall");
  });
});

describe("settleToolCallBlock", () => {
  const running: ToolCallBlock = {
    type: "toolCall",
    id: "call_1",
    name: "readFile",
    argumentsJson: '{"path":"a.md"}',
    status: "running",
  };

  test("settles the matching block to done on a non-null result", () => {
    const fileResult = { content: "content", startLine: 1, endLine: 1, totalLines: 1 };
    const blocks = settleToolCallBlock([running], {
      id: "call_1",
      result: { tool: "file", result: fileResult },
      error: null,
    });
    expect(blocks[0]).toMatchObject({ status: "done", result: { tool: "file", result: fileResult } });
  });

  test("settles the matching block to error on a null result", () => {
    const blocks = settleToolCallBlock([running], { id: "call_1", result: null, error: "not found: a.md" });
    expect(blocks[0]).toMatchObject({ status: "error", errorMessage: "not found: a.md" });
  });

  test("only settles the block whose id matches, among several", () => {
    const other: ToolCallBlock = { ...running, id: "call_2", status: "running" };
    const blocks = settleToolCallBlock([running, other], {
      id: "call_1",
      result: { tool: "file", result: { content: "x", startLine: 1, endLine: 1, totalLines: 1 } },
      error: null,
    });
    expect(blocks[0]).toMatchObject({ status: "done" });
    expect(blocks[1]).toMatchObject({ status: "running" });
  });

  test("is a no-op when no block matches the id", () => {
    const blocks = settleToolCallBlock([running], {
      id: "call_unknown",
      result: { tool: "file", result: { content: "x", startLine: 1, endLine: 1, totalLines: 1 } },
      error: null,
    });
    expect(blocks[0]).toEqual(running);
  });
});

describe("correctTrailingText", () => {
  test("replaces a trailing text block's content", () => {
    const blocks: MessageBlock[] = [{ type: "text", id: "t1", content: "partial" }];
    const corrected = correctTrailingText(blocks, "full authoritative text");
    expect(corrected).toHaveLength(1);
    expect(corrected[0]).toMatchObject({ id: "t1", content: "full authoritative text" });
  });

  test("appends a new text block when trailing is a tool call and text is non-empty", () => {
    const toolCall: ToolCallBlock = {
      type: "toolCall",
      id: "call_1",
      name: "readFile",
      argumentsJson: "{}",
      status: "done",
    };
    const corrected = correctTrailingText([toolCall], "final answer");
    expect(corrected).toHaveLength(2);
    expect(corrected[1]).toMatchObject({ type: "text", content: "final answer" });
  });

  test("is a no-op when trailing is a tool call and text is empty", () => {
    const toolCall: ToolCallBlock = {
      type: "toolCall",
      id: "call_1",
      name: "readFile",
      argumentsJson: "{}",
      status: "done",
    };
    const corrected = correctTrailingText([toolCall], "");
    expect(corrected).toEqual([toolCall]);
  });
});

describe("correctTrailingReasoning", () => {
  test("replaces a trailing reasoning block's content", () => {
    const blocks: MessageBlock[] = [{ type: "reasoning", id: "r1", content: "partial thought" }];
    const corrected = correctTrailingReasoning(blocks, "full authoritative reasoning");
    expect(corrected).toHaveLength(1);
    expect(corrected[0]).toMatchObject({ id: "r1", content: "full authoritative reasoning" });
  });

  test("is a no-op when trailing is not a reasoning block, even if reasoning text is non-empty", () => {
    const text: MessageBlock = { type: "text", id: "t1", content: "the answer" };
    const corrected = correctTrailingReasoning([text], "some reasoning that arrived late");
    expect(corrected).toEqual([text]);
  });

  test("is a no-op on an empty blocks array", () => {
    expect(correctTrailingReasoning([], "reasoning")).toEqual([]);
  });
});

describe("markRunningToolCallsAsInterrupted", () => {
  test("flips only running blocks to error, leaving done/error untouched", () => {
    const running: ToolCallBlock = {
      type: "toolCall",
      id: "call_1",
      name: "readFile",
      argumentsJson: "{}",
      status: "running",
    };
    const done: ToolCallBlock = {
      type: "toolCall",
      id: "call_2",
      name: "listFiles",
      argumentsJson: "{}",
      status: "done",
      result: { tool: "fileList", result: [] },
    };
    const text: MessageBlock = { type: "text", id: "t1", content: "hi" };

    const swept = markRunningToolCallsAsInterrupted([text, done, running]);
    expect(swept[0]).toBe(text);
    expect(swept[1]).toBe(done);
    expect(swept[2]).toMatchObject({ status: "error" });
  });
});

describe("flattenBlocksToText / chatMessageToPlainText", () => {
  test("joins multiple text blocks with a blank line, skipping tool-call blocks", () => {
    const blocks: MessageBlock[] = [
      { type: "text", id: "t1", content: "Let me check that." },
      { type: "toolCall", id: "call_1", name: "readFile", argumentsJson: "{}", status: "done" },
      { type: "text", id: "t2", content: "Based on the file, here's the answer." },
    ];
    expect(flattenBlocksToText(blocks)).toBe("Let me check that.\n\nBased on the file, here's the answer.");
  });

  test("skips empty text blocks", () => {
    const blocks: MessageBlock[] = [
      { type: "text", id: "t1", content: "" },
      { type: "text", id: "t2", content: "real content" },
    ];
    expect(flattenBlocksToText(blocks)).toBe("real content");
  });

  test("chatMessageToPlainText passes user content through unchanged", () => {
    const message: ChatMessage = { id: "m1", role: "user", content: "hello there" };
    expect(chatMessageToPlainText(message)).toBe("hello there");
  });

  test("chatMessageToPlainText flattens an assistant message's blocks", () => {
    const message: ChatMessage = {
      id: "m1",
      role: "assistant",
      blocks: [{ type: "text", id: "t1", content: "the answer" }],
    };
    expect(chatMessageToPlainText(message)).toBe("the answer");
  });
});

describe("groupBlocksForRender", () => {
  const pending = (id: string, name: string, groupId: string): ToolCallBlock => ({
    type: "toolCall",
    id,
    name,
    argumentsJson: "{}",
    status: "pendingApproval",
    approvalGroupId: groupId,
  });

  test("collapses a run of pending requestArtifact calls into one artifact card", () => {
    const grouped = groupBlocksForRender([
      pending("a1", "requestArtifact", "g1"),
      pending("a2", "requestArtifact", "g1"),
    ]);
    expect(grouped).toHaveLength(1);
    expect(grouped[0]!.kind).toBe("artifactGroup");
  });

  test("keeps artifact, ask and approval cards apart even in one round", () => {
    // Each card kind gets its own group id in `collectDecisions`, so a
    // mixed round renders as three cards rather than one incoherent group.
    const grouped = groupBlocksForRender([
      pending("q1", "askUser", "ask"),
      pending("a1", "requestArtifact", "artifact"),
      pending("w1", "writeFile", "approve"),
    ]);
    expect(grouped.map((g) => g.kind)).toEqual(["askGroup", "artifactGroup", "approvalGroup"]);
  });

  test("a settled artifact call is an ordinary block, not a card", () => {
    const settled: ToolCallBlock = {
      type: "toolCall",
      id: "a1",
      name: "requestArtifact",
      argumentsJson: "{}",
      status: "done",
    };
    const grouped = groupBlocksForRender([settled]);
    expect(grouped).toEqual([{ kind: "single", block: settled }]);
  });

  test("text around a card passes through in order", () => {
    const text: MessageBlock = { type: "text", id: "t1", content: "Соберём запрос." };
    const grouped = groupBlocksForRender([text, pending("a1", "requestArtifact", "g1")]);
    expect(grouped.map((g) => g.kind)).toEqual(["single", "artifactGroup"]);
  });
});
