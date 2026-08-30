import { describe, expect, test } from "bun:test";
import {
  appendDeltaToBlocks,
  appendReasoningDeltaToBlocks,
  appendSteerBlock,
  appendPendingApprovalBlock,
  appendToolCallBlock,
  applyToolCallDelta,
  chatMessageToPlainText,
  correctTrailingReasoning,
  correctTrailingText,
  flattenBlocksToText,
  groupBlocksForRender,
  lastBlockShowsLiveProgress,
  markRunningToolCallsAsInterrupted,
  mergeInterleavedStreamBlocks,
  openStreamingBlockIds,
  searchIsDegraded,
  toolLedger,
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

  test("a subsequent text delta opens a new text block below the reasoning one", () => {
    const reasoning = appendReasoningDeltaToBlocks([], "thinking...");
    const withText = appendDeltaToBlocks(reasoning, "the answer");
    expect(withText).toHaveLength(2);
    expect(withText[0]).toMatchObject({ type: "reasoning", content: "thinking..." });
    expect(withText[1]).toMatchObject({ type: "text", content: "the answer" });
  });
});

describe("interleaved reasoning and text deltas", () => {
  // Some providers don't finish thinking before answering — they alternate
  // `reasoning_content` and `content` chunk by chunk. Matching only the
  // trailing block used to open a brand-new block per chunk, shredding one
  // answer into hundreds of tiny blocks with a "thinking" card between each.
  test("keep growing two blocks, not one per chunk", () => {
    let blocks: MessageBlock[] = [];
    for (const [reasoning, text] of [
      ["Let me ", "Сейчас "],
      ["check the ", "проверю "],
      ["standards.", "документацию."],
    ] as const) {
      blocks = appendReasoningDeltaToBlocks(blocks, reasoning);
      blocks = appendDeltaToBlocks(blocks, text);
    }
    expect(blocks).toHaveLength(2);
    expect(blocks[0]).toMatchObject({ type: "reasoning", content: "Let me check the standards." });
    expect(blocks[1]).toMatchObject({ type: "text", content: "Сейчас проверю документацию." });
  });

  test("both blocks count as live progress, so no extra thinking card appears", () => {
    const blocks = appendDeltaToBlocks(appendReasoningDeltaToBlocks([], "hmm"), "answer");
    expect(lastBlockShowsLiveProgress(blocks)).toBe(true);
    expect(openStreamingBlockIds(blocks).size).toBe(2);
  });

  test("a tool call closes both streams — the next deltas open fresh blocks", () => {
    const toolCall: ToolCallBlock = {
      type: "toolCall",
      id: "call_1",
      name: "readFile",
      argumentsJson: "{}",
      status: "done",
    };
    const before = appendDeltaToBlocks(appendReasoningDeltaToBlocks([], "hmm"), "prose");
    const after = appendReasoningDeltaToBlocks(appendDeltaToBlocks([...before, toolCall], "more"), "again");
    expect(after.map((b) => b.type)).toEqual(["reasoning", "text", "toolCall", "text", "reasoning"]);
    expect(openStreamingBlockIds(after)).toEqual(new Set([after[3]!.id, after[4]!.id]));
  });

  test("mergeInterleavedStreamBlocks folds a shredded stored message back together", () => {
    const toolCall: ToolCallBlock = {
      type: "toolCall",
      id: "call_1",
      name: "readFile",
      argumentsJson: "{}",
      status: "done",
    };
    const shredded: MessageBlock[] = [
      { type: "reasoning", id: "r1", content: "Let me " },
      { type: "text", id: "t1", content: "Сейчас " },
      { type: "reasoning", id: "r2", content: "check." },
      { type: "text", id: "t2", content: "проверю." },
      toolCall,
      { type: "text", id: "t3", content: "Гото" },
      { type: "reasoning", id: "r3", content: "done" },
      { type: "text", id: "t4", content: "во." },
    ];
    const merged = mergeInterleavedStreamBlocks(shredded);
    expect(merged).toEqual([
      { type: "reasoning", id: "r1", content: "Let me check." },
      { type: "text", id: "t1", content: "Сейчас проверю." },
      toolCall,
      { type: "text", id: "t3", content: "Готово." },
      { type: "reasoning", id: "r3", content: "done" },
    ]);
  });

  test("mergeInterleavedStreamBlocks returns an untouched conversation as-is", () => {
    const blocks: MessageBlock[] = [
      { type: "reasoning", id: "r1", content: "thinking" },
      { type: "text", id: "t1", content: "the answer" },
    ];
    expect(mergeInterleavedStreamBlocks(blocks)).toBe(blocks);
  });

  test("correctTrailingText fixes up the round's open text block, not just a trailing one", () => {
    const blocks = appendReasoningDeltaToBlocks(appendDeltaToBlocks([], "partia"), "still thinking");
    const corrected = correctTrailingText(blocks, "partial answer, made whole");
    expect(corrected).toHaveLength(2);
    expect(corrected[0]).toMatchObject({ type: "text", content: "partial answer, made whole" });
  });

  test("correctTrailingReasoning fixes up the round's open reasoning block", () => {
    const blocks = appendDeltaToBlocks(appendReasoningDeltaToBlocks([], "partia"), "the answer");
    const corrected = correctTrailingReasoning(blocks, "partial thought, made whole");
    expect(corrected).toHaveLength(2);
    expect(corrected[0]).toMatchObject({ type: "reasoning", content: "partial thought, made whole" });
  });
});

describe("applyToolCallDelta", () => {
  test("opens a running block when the id is new", () => {
    const blocks = applyToolCallDelta([], {
      id: "call_1",
      name: "visualize",
      argumentsJson: '{"title":"',
    });
    expect(blocks).toEqual([
      {
        type: "toolCall",
        id: "call_1",
        name: "visualize",
        argumentsJson: '{"title":"',
        status: "running",
      },
    ]);
  });

  test("rebands a pending:index block onto the real id when it arrives", () => {
    const pending = applyToolCallDelta([], {
      id: "pending:0",
      name: "visualize",
      argumentsJson: '{"source":"flow',
    });
    const named = applyToolCallDelta(pending, {
      id: "call_1",
      name: "visualize",
      argumentsJson: '{"source":"flowchart"}',
    });
    expect(named).toHaveLength(1);
    expect(named[0]).toMatchObject({ id: "call_1", argumentsJson: '{"source":"flowchart"}' });
  });

  test("grows arguments on the existing block without changing its status", () => {
    const pending = appendPendingApprovalBlock([], {
      id: "call_1",
      name: "visualize",
      argumentsJson: "{}",
      approvalGroupId: "g1",
    });
    const grown = applyToolCallDelta(pending, {
      id: "call_1",
      name: "visualize",
      argumentsJson: '{"source":"flowchart"}',
    });
    expect(grown).toHaveLength(1);
    expect(grown[0]).toMatchObject({
      status: "pendingApproval",
      argumentsJson: '{"source":"flowchart"}',
      approvalGroupId: "g1",
    });
  });
});

describe("lastBlockShowsLiveProgress", () => {
  test("false on an empty transcript or after a settled tool call", () => {
    expect(lastBlockShowsLiveProgress([])).toBe(false);
    expect(
      lastBlockShowsLiveProgress([
        { type: "toolCall", id: "c1", name: "readFile", argumentsJson: "{}", status: "done" },
      ]),
    ).toBe(false);
  });

  test("true while text, reasoning, or a running tool is the tail", () => {
    expect(lastBlockShowsLiveProgress([{ type: "text", id: "t", content: "…" }])).toBe(true);
    expect(lastBlockShowsLiveProgress([{ type: "reasoning", id: "r", content: "план" }])).toBe(true);
    expect(
      lastBlockShowsLiveProgress([
        { type: "toolCall", id: "c1", name: "visualize", argumentsJson: "{}", status: "running" },
      ]),
    ).toBe(true);
  });

  test("an empty text block does not count as progress", () => {
    expect(lastBlockShowsLiveProgress([{ type: "text", id: "t", content: "" }])).toBe(false);
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

  test("overwrites arguments on a block that streamed in first", () => {
    const streamed = applyToolCallDelta([], {
      id: "call_1",
      name: "visualize",
      argumentsJson: '{"source":"flow',
    });
    const started = appendToolCallBlock(streamed, {
      id: "call_1",
      name: "visualize",
      argumentsJson: '{"source":"flowchart TD"}',
    });
    expect(started).toHaveLength(1);
    expect(started[0]).toMatchObject({
      status: "running",
      argumentsJson: '{"source":"flowchart TD"}',
    });
  });

  test("rebands a pending:index stream block onto the real execution id", () => {
    const streamed = applyToolCallDelta([], {
      id: "pending:0",
      name: "visualize",
      argumentsJson: '{"source":"flow',
    });
    const started = appendToolCallBlock(streamed, {
      id: "call_1",
      name: "visualize",
      argumentsJson: '{"source":"flowchart TD"}',
    });
    expect(started).toHaveLength(1);
    expect(started[0]).toMatchObject({ id: "call_1", status: "running" });
  });
});

describe("appendPendingApprovalBlock", () => {
  test("transitions a streamed-in running block instead of duplicating it", () => {
    const streamed = applyToolCallDelta([], {
      id: "call_1",
      name: "writeFile",
      argumentsJson: '{"path":"a.md"}',
    });
    const paused = appendPendingApprovalBlock(streamed, {
      id: "call_1",
      name: "writeFile",
      argumentsJson: '{"path":"a.md"}',
      deadlineAt: 1,
      approvalGroupId: "g1",
    });
    expect(paused).toHaveLength(1);
    expect(paused[0]).toMatchObject({
      status: "pendingApproval",
      deadlineAt: 1,
      approvalGroupId: "g1",
    });
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
  test("keeps applied steering in replay text", () => {
    const blocks = appendSteerBlock(
      [{ type: "text", id: "t1", content: "Проверяю." }],
      "Проверь ru locale",
    );

    expect(flattenBlocksToText(blocks)).toBe(
      "Проверяю.\n\n[Уточнение от пользователя, не новое задание — учти в текущей работе]: Проверь ru locale",
    );
  });

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

describe("searchIsDegraded", () => {
  const searchCall = (degraded: string | null, status: ToolCallBlock["status"] = "done"): ToolCallBlock => ({
    type: "toolCall",
    id: `call_${degraded ?? "ok"}_${Math.random()}`,
    name: "semanticSearch",
    argumentsJson: '{"query":"x"}',
    status,
    result: {
      tool: "semanticSearchResults",
      result: {
        matches: [],
        meta: { tiersUsed: ["symbol"], symbolHits: 0, extractedTokens: [], weak: false, hint: null, degraded },
      },
    },
  });

  const withBlocks = (id: string, blocks: MessageBlock[]): ChatMessage => ({
    id,
    role: "assistant",
    blocks,
  });

  test("false for a conversation that never searched", () => {
    expect(searchIsDegraded([])).toBe(false);
    expect(searchIsDegraded([withBlocks("a", [{ type: "text", id: "t", content: "hi" }])])).toBe(false);
  });

  test("true while the newest search ran without the semantic tier", () => {
    expect(searchIsDegraded([withBlocks("a", [searchCall("провайдер недоступен")])])).toBe(true);
  });

  test("clears once a later search succeeds", () => {
    const messages = [
      withBlocks("a", [searchCall("провайдер недоступен")]),
      withBlocks("b", [searchCall(null)]),
    ];
    expect(searchIsDegraded(messages)).toBe(false);
  });

  test("an unfinished search does not clear a standing degradation", () => {
    const messages = [
      withBlocks("a", [searchCall("провайдер недоступен")]),
      withBlocks("b", [searchCall(null, "running")]),
    ];
    expect(searchIsDegraded(messages)).toBe(true);
  });

  test("only the newest search counts, even within one turn", () => {
    const messages = [
      withBlocks("a", [searchCall("провайдер недоступен"), searchCall(null)]),
    ];
    expect(searchIsDegraded(messages)).toBe(false);
  });
});

describe("toolLedger", () => {
  const call = (
    name: string,
    args: Record<string, unknown>,
    status: ToolCallBlock["status"] = "done",
  ): ToolCallBlock => ({
    type: "toolCall",
    id: `call_${name}_${JSON.stringify(args)}`,
    name,
    argumentsJson: JSON.stringify(args),
    status,
  });

  test("records read, changed and deleted paths, changes first", () => {
    const ledger = toolLedger([
      call("readFile", { path: "src/api/AusnController.java" }),
      call("editFile", { path: "docs/fetch.adoc", edits: [] }),
      call("deleteFile", { path: "docs/old.adoc" }),
    ]);
    expect(ledger).toBe(
      "[Файлы, затронутые в этом ходе — изменены: docs/fetch.adoc; удалены: docs/old.adoc; прочитаны: src/api/AusnController.java]",
    );
  });

  test("is empty for a turn that touched no files", () => {
    expect(toolLedger([{ type: "text", id: "t1", content: "just prose" }])).toBe("");
    expect(toolLedger([call("semanticSearch", { query: "x" }), call("todo", { op: "write" })])).toBe("");
  });

  test("ignores calls that did not settle successfully", () => {
    expect(toolLedger([call("readFile", { path: "a.adoc" }, "error")])).toBe("");
    expect(toolLedger([call("readFile", { path: "a.adoc" }, "running")])).toBe("");
    expect(toolLedger([call("writeFile", { path: "a.adoc" }, "pendingApproval")])).toBe("");
  });

  test("dedupes repeated paths and survives unparseable arguments", () => {
    const ledger = toolLedger([
      call("readFile", { path: "a.adoc" }),
      call("readFile", { path: "a.adoc" }),
      { type: "toolCall", id: "c3", name: "readFile", argumentsJson: "{not json", status: "done" },
    ]);
    expect(ledger).toBe("[Файлы, затронутые в этом ходе — прочитаны: a.adoc]");
  });

  test("renders a move as its before → after pair", () => {
    expect(toolLedger([call("move", { path: "old.adoc", newPath: "new.adoc" })])).toBe(
      "[Файлы, затронутые в этом ходе — изменены: old.adoc → new.adoc]",
    );
  });

  test("caps a long research turn and reports how many paths were dropped", () => {
    const reads = Array.from({ length: 45 }, (_, i) => call("readFile", { path: `f${i}.java` }));
    const ledger = toolLedger([call("writeFile", { path: "docs/out.adoc" }), ...reads]);
    // Writes are never the entries dropped, and what survives of the reads
    // is the tail — the files the turn ended on.
    expect(ledger).toContain("изменены: docs/out.adoc");
    expect(ledger).toContain("f44.java");
    expect(ledger).not.toContain("f5.java,");
    expect(ledger).toContain("и ещё 6 файл(ов)");
  });

  test("chatMessageToPlainText appends the ledger so paths survive into the next turn", () => {
    const message: ChatMessage = {
      id: "m1",
      role: "assistant",
      blocks: [
        { type: "text", id: "t1", content: "Смотри AusnTransactionService.java:41." },
        call("readFile", { path: "src/thrift/services/AusnTransactionService.java" }),
      ],
    };
    expect(chatMessageToPlainText(message)).toBe(
      "Смотри AusnTransactionService.java:41.\n\n[Файлы, затронутые в этом ходе — прочитаны: src/thrift/services/AusnTransactionService.java]",
    );
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
