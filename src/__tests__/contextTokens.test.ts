import { describe, expect, test } from "bun:test";
import {
  chatMessageToPlainText,
  estimateMessageContextTokens,
  type ChatMessage,
  type MessageBlock,
} from "../lib/chatBlocks";
import { estimateTokenCount, estimateTokensFromChars } from "../lib/tokens";
import { estimateToolSchemaTokens } from "../lib/assistantConfig";

const FILE_BODY = "x".repeat(4000);

/** An assistant turn that read a file: prose around one settled `readFile`
 * whose result carries the whole file body. */
function turnWithFileRead(streaming: boolean): ChatMessage {
  const blocks: MessageBlock[] = [
    { type: "text", id: "t1", content: "Смотрю файл." },
    {
      type: "toolCall",
      id: "call_1",
      name: "readFile",
      argumentsJson: JSON.stringify({ path: "docs/a.adoc" }),
      status: "done",
      result: {
        tool: "file",
        result: { content: FILE_BODY, startLine: 1, endLine: 200, totalLines: 200 },
      },
    },
    { type: "text", id: "t2", content: "Готово." },
  ];
  return { id: "m1", role: "assistant", blocks, streaming };
}

describe("estimateToolSchemaTokens", () => {
  test("is zero when no tools are advertised", () => {
    expect(estimateToolSchemaTokens([])).toBe(0);
  });

  test("counts the schemas that ride on every request", () => {
    // The real advertised set (24 tools) serializes to ~37 800 characters,
    // about 9 300 tokens — measured off a provider debug log, where leaving
    // it out ran the whole estimate a stable ~36% under the reported
    // `promptTokens`. This fixture is the same order of magnitude.
    const defs = Array.from({ length: 24 }, (_, i) => ({
      name: `tool${i}`,
      description: "x".repeat(800),
      parameters: { type: "object", properties: { path: { type: "string" } } },
    }));
    const estimated = estimateToolSchemaTokens(defs);
    expect(estimated).toBeGreaterThan(4000);
    // Nothing but the definitions — a bigger advertised set costs more, a
    // narrowed allowlist costs less, which is why this is computed rather
    // than a stored constant.
    expect(estimateToolSchemaTokens(defs.slice(0, 12))).toBeLessThan(estimated);
  });
});

describe("estimateTokensFromChars", () => {
  test("matches estimateTokenCount on the same length", () => {
    expect(estimateTokensFromChars("привет".length)).toBe(estimateTokenCount("привет"));
  });

  test("is zero for an empty or negative count", () => {
    expect(estimateTokensFromChars(0)).toBe(0);
    expect(estimateTokensFromChars(-5)).toBe(0);
  });
});

describe("estimateMessageContextTokens", () => {
  test("a user message is just its content", () => {
    const message: ChatMessage = { id: "m1", role: "user", content: "покажи docs/a.adoc" };
    expect(estimateMessageContextTokens(message)).toBe(estimateTokenCount("покажи docs/a.adoc"));
  });

  test("an in-flight turn counts the tool result the backend keeps in its history", () => {
    const inFlight = turnWithFileRead(true);
    const wireOnly = estimateTokenCount(chatMessageToPlainText(inFlight));

    // The file body alone dwarfs the prose — the whole point of the ring
    // moving during a tool sequence.
    expect(estimateMessageContextTokens(inFlight)).toBeGreaterThan(wireOnly + 900);
  });

  test("a settled turn falls back to what will actually be replayed", () => {
    const settled = turnWithFileRead(false);
    expect(estimateMessageContextTokens(settled)).toBe(estimateTokenCount(chatMessageToPlainText(settled)));
  });

  test("a running call counts its arguments but has no result to count yet", () => {
    const args = JSON.stringify({ path: "docs/a.adoc", query: "q".repeat(400) });
    const message: ChatMessage = {
      id: "m1",
      role: "assistant",
      streaming: true,
      blocks: [{ type: "toolCall", id: "call_1", name: "semanticSearch", argumentsJson: args, status: "running" }],
    };
    const estimate = estimateMessageContextTokens(message);
    expect(estimate).toBeGreaterThan(estimateTokensFromChars(args.length));
    // Nothing beyond the arguments plus the small fixed wire overhead.
    expect(estimate).toBeLessThan(estimateTokensFromChars(args.length + 200));
  });

  test("an errored call counts its message instead of a result", () => {
    const base: MessageBlock = {
      type: "toolCall",
      id: "call_1",
      name: "readFile",
      argumentsJson: "{}",
      status: "error",
      errorMessage: "e".repeat(400),
    };
    const withError: ChatMessage = { id: "m1", role: "assistant", streaming: true, blocks: [base] };
    const withoutError: ChatMessage = {
      id: "m1",
      role: "assistant",
      streaming: true,
      blocks: [{ ...base, errorMessage: "" }],
    };
    expect(estimateMessageContextTokens(withError)).toBeGreaterThan(estimateMessageContextTokens(withoutError));
  });

  test("reasoning counts even though it is never replayed across turns", () => {
    const reasoning = "р".repeat(800);
    const withReasoning: ChatMessage = {
      id: "m1",
      role: "assistant",
      streaming: true,
      blocks: [
        { type: "reasoning", id: "r1", content: reasoning },
        { type: "text", id: "t1", content: "Ответ." },
      ],
    };
    const withoutReasoning: ChatMessage = {
      id: "m1",
      role: "assistant",
      streaming: true,
      blocks: [{ type: "text", id: "t1", content: "Ответ." }],
    };
    expect(estimateMessageContextTokens(withReasoning) - estimateMessageContextTokens(withoutReasoning)).toBe(
      estimateTokensFromChars(reasoning.length),
    );
  });

  test("survives arguments that are not valid JSON", () => {
    const message: ChatMessage = {
      id: "m1",
      role: "assistant",
      streaming: true,
      blocks: [
        { type: "toolCall", id: "call_1", name: "readFile", argumentsJson: '{"path": "docs/a', status: "running" },
      ],
    };
    expect(estimateMessageContextTokens(message)).toBeGreaterThan(0);
  });

  test("survives a result that cannot be serialized", () => {
    const circular: Record<string, unknown> = {};
    circular.self = circular;
    const message: ChatMessage = {
      id: "m1",
      role: "assistant",
      streaming: true,
      blocks: [
        {
          type: "toolCall",
          id: "call_1",
          name: "readFile",
          argumentsJson: "{}",
          status: "done",
          // Deliberately malformed — a real `ToolResult` is always
          // JSON-serializable, this only proves the estimate never throws
          // inside a render.
          result: circular as never,
        },
      ],
    };
    expect(estimateMessageContextTokens(message)).toBeGreaterThan(0);
  });
});
