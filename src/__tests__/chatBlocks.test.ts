import { describe, expect, test } from "bun:test";
import {
  appendDeltaToBlocks,
  appendReasoningDeltaToBlocks,
  appendSteerBlock,
  appendPendingApprovalBlock,
  appendToolCallBlock,
  applyToolCallDelta,
  chatMessageToPlainText,
  closeOpenBlocks,
  correctRoundText,
  correctTrailingReasoning,
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
import { APPROVAL_TIMED_OUT_ERROR, TOOL_DENIED_BY_USER, type ToolResult } from "../lib/aiTools";

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

  test("mergeInterleavedStreamBlocks removes persisted adjacent closed duplicates", () => {
    const duplicated: MessageBlock[] = [
      { type: "text", id: "t1", content: "Готово.", closed: true },
      { type: "text", id: "t2", content: "Готово.", closed: true },
    ];
    expect(mergeInterleavedStreamBlocks(duplicated)).toEqual([
      { type: "text", id: "t1", content: "Готово.", closed: true },
    ]);
  });

  test("mergeInterleavedStreamBlocks preserves equal closed text across a real boundary", () => {
    const boundary: MessageBlock = { type: "steer", id: "s1", text: "повтори" };
    const repeated: MessageBlock[] = [
      { type: "text", id: "t1", content: "Готово.", closed: true },
      boundary,
      { type: "text", id: "t2", content: "Готово.", closed: true },
    ];
    expect(mergeInterleavedStreamBlocks(repeated)).toBe(repeated);
  });

  test("correctTrailingReasoning fixes up the round's open reasoning block", () => {
    const blocks = appendDeltaToBlocks(appendReasoningDeltaToBlocks([], "partia"), "the answer");
    const corrected = correctTrailingReasoning(blocks, "partial thought, made whole");
    expect(corrected).toHaveLength(2);
    expect(corrected[0]).toMatchObject({ type: "reasoning", content: "partial thought, made whole" });
  });
});

describe("correctRoundText", () => {
  const call = { id: "call_1", name: "readFile", argumentsJson: "{}" };

  test("reaches the round's text block through the tool calls that follow it", () => {
    // The shape that used to be unreachable: prose, then the call it led to.
    const blocks = appendToolCallBlock(appendDeltaToBlocks([], "Смотрю фай"), call);
    const corrected = correctRoundText(blocks, "Смотрю файл конфигурации.");
    expect(corrected[0]).toMatchObject({ type: "text", content: "Смотрю файл конфигурации." });
    expect(corrected).toHaveLength(2);
  });

  test("reaches it through several parallel calls in the same round", () => {
    let blocks = appendDeltaToBlocks([], "Читаю об");
    blocks = appendToolCallBlock(blocks, call);
    blocks = appendToolCallBlock(blocks, { ...call, id: "call_2" });
    const corrected = correctRoundText(blocks, "Читаю оба файла.");
    expect(corrected[0]).toMatchObject({ type: "text", content: "Читаю оба файла." });
  });

  test("corrects a round that has not called anything yet", () => {
    const corrected = correctRoundText(appendDeltaToBlocks([], "Гото"), "Готово.");
    expect(corrected[0]).toMatchObject({ type: "text", content: "Готово." });
  });

  test("applying the same authoritative text twice is idempotent", () => {
    const once = correctRoundText(
      appendDeltaToBlocks(appendToolCallBlock([], call), "Смотрю фай"),
      "Смотрю файл конфигурации.",
    );
    expect(correctRoundText(once, "Смотрю файл конфигурации.")).toBe(once);
    expect(once.filter((b) => b.type === "text")).toHaveLength(1);
  });

  test("recovers wholly lost prose that repeats an earlier round across a tool boundary", () => {
    const firstRound = correctRoundText(appendDeltaToBlocks([], "Готово."), "Готово.");
    const secondRound = correctRoundText(appendToolCallBlock(firstRound, call), "Готово.");
    expect(secondRound.filter((b) => b.type === "text").map((b) => b.type === "text" && b.content)).toEqual([
      "Готово.",
      "Готово.",
    ]);
  });

  test("recovers a lost round in front of its calls without touching the closed one", () => {
    // A closed block is by definition not the round reporting now, so this
    // round wrote nothing that survived: its deltas were all dropped, and
    // `text` is the only copy left. Rescuing it is the whole point of the
    // event — but it goes in beside the earlier round's prose, never over it.
    const blocks = appendToolCallBlock(closeOpenBlocks(appendDeltaToBlocks([], "Первый раунд.")), call);
    const corrected = correctRoundText(blocks, "Второй раунд.");
    expect(corrected.map((b) => b.type)).toEqual(["text", "text", "toolCall"]);
    expect(corrected[0]).toMatchObject({ content: "Первый раунд.", closed: true });
    expect(corrected[1]).toMatchObject({ content: "Второй раунд.", closed: true });
  });

  test("stops at a steer instead of reaching across it", () => {
    const blocks = appendSteerBlock(appendDeltaToBlocks([], "До вмешательства."), "подожди");
    expect(correctRoundText(blocks, "не сюда")).toBe(blocks);
  });

  test("an empty text never blanks a block that has content", () => {
    const blocks = appendDeltaToBlocks([], "Настоящий ответ.");
    expect(correctRoundText(blocks, "")).toBe(blocks);
  });

  test("inserts the prose before the round's calls when every delta was lost", () => {
    const blocks = appendToolCallBlock([], call);
    const corrected = correctRoundText(blocks, "Проза, потерянная целиком.");
    expect(corrected).toHaveLength(2);
    expect(corrected[0]).toMatchObject({ type: "text", content: "Проза, потерянная целиком." });
    expect(corrected[1]).toMatchObject({ type: "toolCall" });
  });

  test("reaches the text block past a reasoning block that trails the tool calls", () => {
    // The regression: an interleaving provider can leave a reasoning block
    // after the round's tool calls. Treating that as a boundary left the
    // text block unreached, and the round's prose was appended a second
    // time — the transcript showed every paragraph twice.
    let blocks = appendDeltaToBlocks([], "Начну с чтен");
    blocks = appendToolCallBlock(blocks, call);
    blocks = appendReasoningDeltaToBlocks(blocks, "ещё думаю");
    const corrected = correctRoundText(blocks, "Начну с чтения файлов.");
    expect(corrected).toHaveLength(3);
    expect(corrected.filter((b) => b.type === "text")).toHaveLength(1);
    expect(corrected[0]).toMatchObject({ type: "text", content: "Начну с чтения файлов." });
  });

  test("reaches it past reasoning interleaved on both sides of the prose", () => {
    let blocks = appendReasoningDeltaToBlocks([], "сначала");
    blocks = appendDeltaToBlocks(blocks, "част");
    blocks = appendToolCallBlock(blocks, call);
    blocks = appendReasoningDeltaToBlocks(blocks, "потом");
    const corrected = correctRoundText(blocks, "частичный ответ, целиком");
    expect(corrected.filter((b) => b.type === "text")).toHaveLength(1);
    expect(corrected.find((b) => b.type === "text")).toMatchObject({
      content: "частичный ответ, целиком",
    });
  });

  test("folds a delta that landed after the tool call back into the round's prose", () => {
    // The defect, straight off a real transcript: the model emits a trailing
    // "\n" once the tool call's arguments have started streaming, which opens
    // a second text block for the same round. Writing the round's full text
    // into that orphan left the block in front of the call standing, so every
    // paragraph appeared twice — once before the tool-call chip, once after.
    let blocks = appendDeltaToBlocks([], "Смотрю файл конфигурации.");
    blocks = appendToolCallBlock(blocks, call);
    blocks = appendDeltaToBlocks(blocks, "\n");
    const corrected = correctRoundText(blocks, "Смотрю файл конфигурации.\n");
    expect(corrected.map((b) => b.type)).toEqual(["text", "toolCall"]);
    expect(corrected[0]).toMatchObject({
      type: "text",
      content: "Смотрю файл конфигурации.\n",
      closed: true,
    });
  });

  test("folds a round whose prose is split across several of its tool calls", () => {
    let blocks = appendDeltaToBlocks([], "Читаю ");
    blocks = appendToolCallBlock(blocks, call);
    blocks = appendDeltaToBlocks(blocks, "оба ");
    blocks = appendToolCallBlock(blocks, { ...call, id: "call_2" });
    blocks = appendDeltaToBlocks(blocks, "файла.");
    const corrected = correctRoundText(blocks, "Читаю оба файла.");
    expect(corrected.filter((b) => b.type === "text")).toHaveLength(1);
    expect(corrected.map((b) => b.type)).toEqual(["text", "toolCall", "toolCall"]);
    expect(corrected[0]).toMatchObject({ content: "Читаю оба файла." });
  });

  test("leaves the previous round's prose alone while folding this one's", () => {
    let blocks = correctRoundText(
      appendToolCallBlock(appendDeltaToBlocks([], "Первый раунд."), call),
      "Первый раунд.",
    );
    blocks = appendDeltaToBlocks(blocks, "Второй ");
    blocks = appendToolCallBlock(blocks, { ...call, id: "call_2" });
    blocks = appendDeltaToBlocks(blocks, "\n");
    const corrected = correctRoundText(blocks, "Второй раунд.\n");
    expect(corrected.filter((b) => b.type === "text").map((b) => b.type === "text" && b.content)).toEqual([
      "Первый раунд.",
      "Второй раунд.\n",
    ]);
  });

  test("no interleaving of a round's deltas and calls loses or duplicates its prose", () => {
    // What the model is owed on the next turn is exactly what it said, once.
    // The provider accumulates every `content` delta of a round into one
    // string regardless of where the round's tool calls fell among them (see
    // `chat_stream`'s `full`), and that string is what `roundCompleted`
    // reports — so for every possible interleaving, the round's blocks must
    // flatten back to it verbatim.
    const chunks = ["Смотрю ", "файл ", "конфигурации."];
    const full = chunks.join("");
    // Every placement of up to two tool calls among the chunks.
    for (let mask = 0; mask < 1 << (chunks.length + 1); mask++) {
      let blocks: MessageBlock[] = [];
      let calls = 0;
      for (let slot = 0; slot <= chunks.length; slot++) {
        if (mask & (1 << slot)) {
          blocks = appendToolCallBlock(blocks, { ...call, id: `call_${++calls}` });
        }
        if (slot < chunks.length) blocks = appendDeltaToBlocks(blocks, chunks[slot]!);
      }
      const corrected = correctRoundText(blocks, full);
      expect(flattenBlocksToText(corrected)).toBe(full);
      expect(corrected.filter((b) => b.type === "toolCall")).toHaveLength(calls);
    }
  });

  test("a folded round leaves the previous round's replay text untouched", () => {
    // The cross-turn wire projection is `flattenBlocksToText` — a fold that
    // reached into an earlier round would silently rewrite history the model
    // already acted on.
    let blocks = correctRoundText(
      appendToolCallBlock(appendDeltaToBlocks([], "Первый раунд."), call),
      "Первый раунд.",
    );
    blocks = appendDeltaToBlocks(blocks, "Второй ");
    blocks = appendToolCallBlock(blocks, { ...call, id: "call_2" });
    blocks = appendDeltaToBlocks(blocks, "раунд.");
    expect(flattenBlocksToText(correctRoundText(blocks, "Второй раунд."))).toBe(
      "Первый раунд.\n\nВторой раунд.",
    );
  });

  test("puts recovered prose after the reasoning it followed", () => {
    const blocks = appendToolCallBlock(appendReasoningDeltaToBlocks([], "думаю"), call);
    const corrected = correctRoundText(blocks, "Ответ.");
    expect(corrected.map((b) => b.type)).toEqual(["reasoning", "text", "toolCall"]);
  });
});

describe("round boundaries", () => {
  test("a new round's prose opens its own block instead of extending the last one", () => {
    // The exact defect: a round that ended in an answer, followed by another
    // round (an app-authored note, or a steer typed mid-stream), used to
    // produce "…итог — HTTP 200.Теперь доступ к коду есть" in one block.
    const firstRound = appendDeltaToBlocks([], "…итог — HTTP 200.");
    const closed = closeOpenBlocks(firstRound);
    const secondRound = appendDeltaToBlocks(closed, "Теперь доступ к коду есть");

    expect(secondRound.map((b) => b.type)).toEqual(["text", "text"]);
    expect(secondRound[0]).toMatchObject({ content: "…итог — HTTP 200.", closed: true });
    expect(secondRound[1]).toMatchObject({ content: "Теперь доступ к коду есть" });
  });

  test("it closes reasoning too, and nothing is live afterwards", () => {
    const blocks = appendDeltaToBlocks(appendReasoningDeltaToBlocks([], "hmm"), "answer");
    const closed = closeOpenBlocks(blocks);
    expect(openStreamingBlockIds(closed).size).toBe(0);
    expect(appendReasoningDeltaToBlocks(closed, "next round").map((b) => b.type)).toEqual([
      "reasoning",
      "text",
      "reasoning",
    ]);
  });

  test("closing twice changes nothing and keeps the same array", () => {
    const closed = closeOpenBlocks(appendDeltaToBlocks([], "готово"));
    expect(closeOpenBlocks(closed)).toBe(closed);
    expect(closeOpenBlocks([])).toEqual([]);
  });

  test("a corrected round's block is closed, so a later round cannot claim it", () => {
    // `closeOpenBlocks` can never reach a text block sitting behind a tool
    // call, so `roundCompleted` closing it is the only boundary that round
    // ever gets — without it a later round that called a tool and lost every
    // delta would overwrite this round's answer with its own text.
    const call = { id: "call_1", name: "readFile", argumentsJson: "{}" };
    const round1 = correctRoundText(
      appendToolCallBlock(appendDeltaToBlocks([], "перв"), call),
      "первый раунд",
    );
    expect(round1[0]).toMatchObject({ type: "text", content: "первый раунд", closed: true });
    const round2 = correctRoundText(appendToolCallBlock(round1, { ...call, id: "call_2" }), "второй раунд");
    expect(round2.filter((b) => b.type === "text").map((b) => b.type === "text" && b.content)).toEqual([
      "первый раунд",
      "второй раунд",
    ]);
  });

  test("a stored transcript is not re-glued across a round boundary on load", () => {
    const stored: MessageBlock[] = [
      { type: "text", id: "t1", content: "первый раунд", closed: true },
      { type: "text", id: "t2", content: "второй раунд" },
    ];
    expect(mergeInterleavedStreamBlocks(stored)).toBe(stored);
  });

  test("a shredded round still folds together up to its boundary", () => {
    const stored: MessageBlock[] = [
      { type: "text", id: "t1", content: "пер" },
      { type: "text", id: "t2", content: "вый", closed: true },
      { type: "text", id: "t3", content: "второй" },
    ];
    expect(mergeInterleavedStreamBlocks(stored)).toEqual([
      { type: "text", id: "t1", content: "первый", closed: true },
      { type: "text", id: "t3", content: "второй" },
    ]);
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
    extra: Partial<ToolCallBlock> = {},
  ): ToolCallBlock => ({
    type: "toolCall",
    id: `call_${name}_${JSON.stringify(args)}_${status}_${JSON.stringify(extra)}`,
    name,
    argumentsJson: JSON.stringify(args),
    status,
    ...extra,
  });

  const grepHits = (n: number): ToolResult => ({
    tool: "grepResults",
    result: {
      matches: Array.from({ length: n }, (_, i) => ({ path: `f${i}.adoc`, line: i, text: "hit" })),
      truncated: false,
    },
  });

  const semanticHits = (n: number): ToolResult => ({
    tool: "semanticSearchResults",
    result: Array.from({ length: n }, (_, i) => ({
      path: `f${i}.adoc`,
      snippet: "hit",
      score: 1,
      startByte: 0,
      endByte: 1,
      qualifiedName: null,
      source: "lexical" as const,
    })),
  });

  // A validation run names a path but shows no file, so it is in no tool
  // category — before its own arm it left the ledger empty and the next
  // turn re-ran the identical check.
  test("records a check and how much it found", () => {
    const ledger = toolLedger([
      call("check", { path: "docs/a.adoc" }, "done", {
        result: {
          tool: "checkResults",
          result: {
            kind: "asciidoc",
            diagnostics: [
              { kind: "parseError", message: "x", document: "docs/a.adoc", line: 1, column: 1, severity: "error" },
              { kind: "missingInclude", message: "y", document: "docs/a.adoc", line: 2, column: 1, severity: "error" },
            ],
            truncated: false,
          },
        } as ToolResult,
      }),
      call("check", { path: "docs/b.adoc" }, "done", {
        result: {
          tool: "checkResults",
          result: { kind: "asciidoc", diagnostics: [], truncated: false },
        } as ToolResult,
      }),
    ]);
    expect(ledger).toContain("проверено: docs/a.adoc (2), docs/b.adoc (без замечаний)");
  });

  // Which artifact the work is about cannot be recovered from a list of
  // every artifact in the project — the content still can, with one call.
  test("records the artifact a turn worked with, by id and title", () => {
    const ledger = toolLedger([
      call("artifact", { op: "read", id: "art_1" }, "done", {
        result: {
          tool: "artifact",
          result: {
            artifact: { id: "art_1", kind: "httpRequest", title: "Перевод средств" },
            rendered: {},
          },
        } as unknown as ToolResult,
      }),
    ]);
    expect(ledger).toContain("артефакты: art_1 «Перевод средств»");
  });

  test("records which templates were fetched", () => {
    const ledger = toolLedger([
      call("getAsciidocTemplates", { ids: ["rest-method", "error-table"] }, "done", {
        result: {
          tool: "asciidocTemplates",
          result: {
            templates: [
              { id: "rest-method", label: "REST", template: "..." },
              { id: "error-table", label: "Ошибки", template: "..." },
            ],
            notFound: [],
          },
        } as unknown as ToolResult,
      }),
    ]);
    expect(ledger).toContain("шаблоны: rest-method, error-table");
  });

  test("a move says how many references it rewrote", () => {
    const ledger = toolLedger([
      call("move", { path: "docs/a.adoc", newPath: "docs/b.adoc" }, "done", {
        result: {
          tool: "moved",
          result: {
            from: "docs/a.adoc",
            to: "docs/b.adoc",
            updatedFiles: [{ path: "docs/x.adoc" }, { path: "docs/y.adoc" }],
          },
        } as unknown as ToolResult,
      }),
    ]);
    expect(ledger).toContain("docs/a.adoc → docs/b.adoc (+2 ссылок)");
  });

  test("a directory created from a template says how much landed in it", () => {
    const ledger = toolLedger([
      call("createDirectory", { path: "docs/newMethod", template: "rest" }, "done", {
        result: {
          tool: "directoryCreated",
          result: {
            path: "docs/newMethod",
            template: "rest",
            createdFiles: ["request.adoc", "response.adoc", "index.adoc"],
          },
        } as ToolResult,
      }),
    ]);
    expect(ledger).toContain("docs/newMethod (+3 файлов)");
  });

  test("records read, changed and deleted paths, changes first", () => {
    const ledger = toolLedger([
      call("readFile", { path: "src/api/AusnController.java" }),
      call("editFile", { path: "docs/fetch.adoc", edits: [] }),
      call("deleteFile", { path: "docs/old.adoc" }),
    ]);
    expect(ledger).toBe(
      "[В этом ходе — изменены: docs/fetch.adoc; удалены: docs/old.adoc; прочитаны: src/api/AusnController.java]",
    );
  });

  test("is empty for a turn that did nothing worth remembering", () => {
    expect(toolLedger([{ type: "text", id: "t1", content: "just prose" }])).toBe("");
    expect(toolLedger([call("todo", { op: "write" })])).toBe("");
    // A search with no recognizable query is not a search anyone can reuse.
    expect(toolLedger([call("semanticSearch", { query: "  " })])).toBe("");
  });

  test("ignores calls still in flight, which have no outcome to record yet", () => {
    expect(toolLedger([call("readFile", { path: "a.adoc" }, "running")])).toBe("");
    expect(toolLedger([call("writeFile", { path: "a.adoc" }, "pendingApproval")])).toBe("");
  });

  test("records searches with their hit counts", () => {
    // The count is the point: a query that found nothing must read as
    // answered, or the next turn runs it again verbatim.
    const ledger = toolLedger([
      call("grep", { pattern: "AusnTransaction" }, "done", { result: grepHits(12) }),
      call("semanticSearch", { query: "подпись патента" }, "done", { result: semanticHits(0) }),
    ]);
    expect(ledger).toBe(
      "[В этом ходе — искали: «AusnTransaction» (grep, найдено 12), «подпись патента» (semanticSearch, ничего не найдено)]",
    );
  });

  test("a search is remembered even when its result is gone", () => {
    expect(toolLedger([call("grep", { pattern: "AusnTransaction" })])).toBe(
      "[В этом ходе — искали: «AusnTransaction» (grep)]",
    );
  });

  test("a grep's path does not turn it into a file the turn read", () => {
    // It searched inside the file, it did not see its contents.
    const ledger = toolLedger([call("grep", { pattern: "patent", path: "docs/api" }, "done", { result: grepHits(1) })]);
    expect(ledger).not.toContain("прочитаны");
    expect(ledger).toContain("искали: «patent» (grep, найдено 1)");
  });

  test("records a refusal apart from a failure, and never as the wrong one", () => {
    const ledger = toolLedger([
      call("writeFile", { path: "docs/x.adoc" }, "error", { errorMessage: TOOL_DENIED_BY_USER }),
      call("askUser", { title: "Уточнить?" }, "error", { errorMessage: TOOL_DENIED_BY_USER }),
      call("requestArtifact", { id: "a" }, "error", { errorMessage: TOOL_DENIED_BY_USER }),
      call("editFile", { path: "docs/y.adoc" }, "error", { errorMessage: APPROVAL_TIMED_OUT_ERROR }),
      call("check", { kind: "standards" }, "error", { errorMessage: "parser crashed" }),
    ]);
    expect(ledger).toBe(
      "[В этом ходе — не выполнено: writeFile docs/x.adoc (отклонено пользователем), " +
        "askUser (пропущено пользователем), requestArtifact (отложено пользователем), " +
        "editFile docs/y.adoc (истекло время на подтверждение), check (ошибка: parser crashed)]",
    );
  });

  test("a refused write is not also counted as a file the turn changed", () => {
    const ledger = toolLedger([
      call("writeFile", { path: "docs/x.adoc" }, "error", { errorMessage: TOOL_DENIED_BY_USER }),
    ]);
    expect(ledger).not.toContain("изменены");
  });

  test("long queries and long errors are cut down to a recognizable stub", () => {
    const ledger = toolLedger([
      call("semanticSearch", { query: "как\n  устроена   ".concat("подпись ".repeat(20)) }, "done", {
        result: semanticHits(1),
      }),
      call("check", { kind: "standards" }, "error", { errorMessage: "x".repeat(200) }),
    ]);
    expect(ledger).toContain("«как устроена подпись");
    expect(ledger).toContain("…»");
    expect(ledger).toContain(`ошибка: ${"x".repeat(80)}…`);
  });

  test("dedupes repeated paths and survives unparseable arguments", () => {
    const ledger = toolLedger([
      call("readFile", { path: "a.adoc" }),
      call("readFile", { path: "a.adoc" }),
      { type: "toolCall", id: "c3", name: "readFile", argumentsJson: "{not json", status: "done" },
    ]);
    expect(ledger).toBe("[В этом ходе — прочитаны: a.adoc]");
  });

  test("renders a move as its before → after pair", () => {
    expect(toolLedger([call("move", { path: "old.adoc", newPath: "new.adoc" })])).toBe(
      "[В этом ходе — изменены: old.adoc → new.adoc]",
    );
  });

  test("searches and failures have their own caps, so paths cannot starve them", () => {
    const reads = Array.from({ length: 45 }, (_, i) => call("readFile", { path: `f${i}.java` }));
    const searches = Array.from({ length: 11 }, (_, i) =>
      call("grep", { pattern: `p${i}` }, "done", { result: grepHits(1) }),
    );
    const ledger = toolLedger([...reads, ...searches]);
    expect(ledger).toContain("«p10»");
    expect(ledger).not.toContain("«p2»");
    expect(ledger).toContain("и ещё 3");
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
      "Смотри AusnTransactionService.java:41.\n\n[В этом ходе — прочитаны: src/thrift/services/AusnTransactionService.java]",
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

describe("groupBlocksForRender — activity runs", () => {
  const call = (
    id: string,
    name = "readFile",
    status: ToolCallBlock["status"] = "done",
  ): ToolCallBlock => ({
    type: "toolCall",
    id,
    name,
    argumentsJson: "{}",
    status,
  });
  const think = (id: string): MessageBlock => ({ type: "reasoning", id, content: "думаю" });
  const say = (id: string): MessageBlock => ({ type: "text", id, content: "Готово." });
  const awaiting = (id: string, name: string, groupId: string): ToolCallBlock => ({
    ...call(id, name, "pendingApproval"),
    approvalGroupId: groupId,
  });

  test("reasoning and tool calls collapse into one activity line", () => {
    const grouped = groupBlocksForRender([
      think("r1"),
      call("t1", "semanticSearch"),
      call("t2"),
      think("r2"),
      call("t3", "semanticSearch"),
    ]);
    expect(grouped).toHaveLength(1);
    expect(grouped[0]!.kind).toBe("activityGroup");
    expect(grouped[0]!.kind === "activityGroup" && grouped[0]!.blocks).toHaveLength(5);
  });

  test("the answer breaks the run instead of being folded into it", () => {
    const grouped = groupBlocksForRender([
      think("r1"),
      call("t1"),
      say("a1"),
      think("r2"),
      call("t2"),
    ]);
    expect(grouped.map((g) => g.kind)).toEqual(["activityGroup", "single", "activityGroup"]);
  });

  test("a lone activity block is not wrapped in a disclosure", () => {
    // One tool call is already one compact line; a group around it would be
    // a control the user has to open to learn nothing new.
    expect(groupBlocksForRender([call("t1")])).toEqual([
      { kind: "single", block: call("t1") },
    ]);
    expect(groupBlocksForRender([think("r1")])).toEqual([
      { kind: "single", block: think("r1") },
    ]);
  });

  test("calls with a card of their own stay full-size entries", () => {
    // A plan, a ticket and a diagram are the turn's output, not machinery.
    const grouped = groupBlocksForRender([
      call("t1"),
      call("p1", "createPlan"),
      call("v1", "visualize"),
      call("t2"),
    ]);
    expect(grouped.map((g) => g.kind)).toEqual(["single", "single", "single", "single"]);
  });

  test("a decision card interrupts the run rather than hiding inside it", () => {
    const grouped = groupBlocksForRender([
      think("r1"),
      call("t1"),
      awaiting("w1", "writeFile", "approve"),
      call("t2"),
      call("t3"),
    ]);
    expect(grouped.map((g) => g.kind)).toEqual([
      "activityGroup",
      "approvalGroup",
      "activityGroup",
    ]);
  });

  test("a failed call is still part of the run, and still countable", () => {
    const grouped = groupBlocksForRender([call("t1"), call("t2", "readFile", "error")]);
    expect(grouped[0]!.kind === "activityGroup" && grouped[0]!.blocks).toHaveLength(2);
  });
});
