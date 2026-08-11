import { describe, expect, test } from "bun:test";
import {
  describeMessageForCompaction,
  formatCompactionNoticeText,
  isCacheValid,
  isCompactionNotice,
  isContextLengthError,
  planCompaction,
  realMessages,
  shouldCompact,
  type CompactionCache,
} from "../lib/contextCompaction";
import type { ChatMessage, MessageBlock, ToolCallBlock } from "../lib/chatBlocks";

function userMsg(id: string, content: string): ChatMessage {
  return { id, role: "user", content };
}

function assistantMsg(id: string, text: string, extraBlocks: MessageBlock[] = []): ChatMessage {
  return {
    id,
    role: "assistant",
    blocks: [{ type: "text", id: `${id}-t`, content: text }, ...extraBlocks],
    streaming: false,
  };
}

function noticeMsg(id: string): ChatMessage {
  return { id, role: "assistant", blocks: [{ type: "text", id: `${id}-t`, content: "notice" }], streaming: false, isCompactionNotice: true };
}

// Alternating user/assistant turns, ids `m0`, `m1`, ... in order.
function conversation(count: number): ChatMessage[] {
  const out: ChatMessage[] = [];
  for (let i = 0; i < count; i++) {
    out.push(i % 2 === 0 ? userMsg(`m${i}`, `user turn ${i}`) : assistantMsg(`m${i}`, `assistant reply ${i}`));
  }
  return out;
}

describe("realMessages / isCompactionNotice", () => {
  test("filters out compaction-notice messages", () => {
    const messages = [userMsg("a", "hi"), noticeMsg("n1"), assistantMsg("b", "hello")];
    expect(realMessages(messages).map((m) => m.id)).toEqual(["a", "b"]);
  });
});

describe("shouldCompact", () => {
  test("false when no contextLimit is configured", () => {
    expect(shouldCompact(100_000, null, conversation(30))).toBe(false);
  });

  test("false below the minimum real-message floor even at a high ratio", () => {
    expect(shouldCompact(9000, 1000, conversation(6))).toBe(false);
  });

  test("false when estimated usage is under the trigger ratio", () => {
    expect(shouldCompact(700, 1000, conversation(30))).toBe(false);
  });

  test("true once usage crosses the trigger ratio with enough messages", () => {
    expect(shouldCompact(850, 1000, conversation(30))).toBe(true);
  });
});

describe("planCompaction", () => {
  test("first pass: summarizes the older messages, keeps the last `keepLast` verbatim", () => {
    const messages = conversation(20);
    const plan = planCompaction(messages, null, 12, null);
    expect(plan).not.toBeNull();
    expect(plan!.toSummarize.map((m) => m.id)).toEqual(messages.slice(0, 8).map((m) => m.id));
    expect(plan!.tail.map((m) => m.id)).toEqual(messages.slice(8).map((m) => m.id));
    expect(plan!.newBoundaryId).toBe("m7");
  });

  test("returns null when there's nothing older than the keep-last window", () => {
    const messages = conversation(10);
    expect(planCompaction(messages, null, 12, null)).toBeNull();
  });

  test("extending an existing cache only considers the segment after the boundary", () => {
    const messages = conversation(30);
    const cache: CompactionCache = { summaryText: "earlier summary", boundaryMessageId: "m9" };
    const plan = planCompaction(messages, cache, 12, null);
    expect(plan).not.toBeNull();
    // segment is m10..m29 (20 messages); keepLast=12 keeps the last 12 (m18..m29) verbatim,
    // summarizes m10..m17.
    expect(plan!.toSummarize.map((m) => m.id)).toEqual(messages.slice(10, 18).map((m) => m.id));
    expect(plan!.newBoundaryId).toBe("m17");
  });

  test("stops early at the first older message mentioning the active file", () => {
    const messages = conversation(20);
    // Make an early message (well before the recency floor) reference the active file.
    messages[3] = userMsg("m3", "please look at docs/guide.adoc again");
    const plan = planCompaction(messages, null, 12, "docs/guide.adoc");
    expect(plan).not.toBeNull();
    // Summarization stops at m3 (exclusive) since it mentions the active file.
    expect(plan!.toSummarize.map((m) => m.id)).toEqual(["m0", "m1", "m2"]);
    expect(plan!.newBoundaryId).toBe("m2");
    // m3 onward (including the recency tail) stays verbatim.
    expect(plan!.tail.map((m) => m.id)).toEqual(messages.slice(3).map((m) => m.id));
  });

  test("a stale boundary id (not found in priorTurns) is treated like no cache at all", () => {
    const messages = conversation(20);
    const cache: CompactionCache = { summaryText: "from another conversation", boundaryMessageId: "does-not-exist" };
    const plan = planCompaction(messages, cache, 12, null);
    expect(plan).not.toBeNull();
    expect(plan!.toSummarize.map((m) => m.id)).toEqual(messages.slice(0, 8).map((m) => m.id));
  });
});

describe("isCacheValid", () => {
  test("false for a null cache", () => {
    expect(isCacheValid(null, conversation(10))).toBe(false);
  });

  test("true when the boundary message is present", () => {
    const messages = conversation(10);
    expect(isCacheValid({ summaryText: "s", boundaryMessageId: "m4" }, messages)).toBe(true);
  });

  test("false when the boundary message belongs to a different conversation", () => {
    const messages = conversation(10);
    expect(isCacheValid({ summaryText: "s", boundaryMessageId: "foreign-id" }, messages)).toBe(false);
  });
});

describe("describeMessageForCompaction", () => {
  test("renders a user message with a role prefix", () => {
    expect(describeMessageForCompaction(userMsg("a", "what does this file do?"))).toBe("User: what does this file do?");
  });

  test("renders an assistant message's text plus a compact line per settled tool call", () => {
    const toolCall: ToolCallBlock = {
      type: "toolCall",
      id: "call_1",
      name: "readFile",
      argumentsJson: "{}",
      status: "done",
      result: { tool: "file", result: { startLine: 1, endLine: 42, totalLines: 42, content: "..." } } as ToolCallBlock["result"],
    };
    const message: ChatMessage = {
      id: "a",
      role: "assistant",
      blocks: [{ type: "text", id: "t1", content: "Reading the file now." }, toolCall],
      streaming: false,
    };
    const out = describeMessageForCompaction(message);
    expect(out).toContain("Assistant: Reading the file now.");
    expect(out).toContain("[tool] readFile ->");
  });
});

describe("formatCompactionNoticeText", () => {
  test("singular phrasing for one message", () => {
    expect(formatCompactionNoticeText(3, 3)).toBe("История сжата (сообщение 3 свёрнуто в резюме)");
  });

  test("range phrasing for several messages", () => {
    expect(formatCompactionNoticeText(1, 24)).toBe("История сжата (сообщения 1–24 свёрнуты в резюме)");
  });
});

describe("isContextLengthError", () => {
  test("matches a realistic OpenAI-compatible error body", () => {
    const message =
      'http status 400: {"error":{"message":"This model\'s maximum context length is 8192 tokens. However, your messages resulted in 9001 tokens.","type":"invalid_request_error"}}';
    expect(isContextLengthError(message)).toBe(true);
  });

  test("matches a context_length_exceeded error code", () => {
    expect(isContextLengthError('{"error":{"code":"context_length_exceeded"}}')).toBe(true);
  });

  test("does not match an unrelated HTTP error", () => {
    expect(isContextLengthError("http status 500: internal server error")).toBe(false);
  });
});
