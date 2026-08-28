import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualLlm from "../lib/llm";
import * as actualAiTools from "../lib/aiTools";
import type { ChatMessage } from "../lib/chatBlocks";
import type { ChatStreamOutcome, PendingApproval, PendingToolCall, ToolCallDecision } from "../lib/llm";

// --- backend doubles -------------------------------------------------------

type Listener<T> = (payload: T) => void;

let deltaListeners: Listener<{ delta: string }>[] = [];
let reasoningListeners: Listener<{ delta: string }>[] = [];
let toolCallListeners: Listener<{ id: string; name: string; arguments: string }>[] = [];
let toolResultListeners: Listener<{ id: string; result: unknown; error: string | null }>[] = [];

/** Outcomes handed back by `streamLlmChat`, then `streamLlmChatResume`. */
let outcomes: ChatStreamOutcome[] = [];
let streamThrows: string | null = null;
let streamCalls: unknown[][] = [];
let resumeCalls: unknown[][] = [];
let cancelCalls = 0;
let autoApprovedTools: string[] = [];
let setAutoApprovedCalls: Array<[string, boolean]> = [];
let onceResponse = "сводка";
/** When set, `streamLlmChat` hangs until the test resolves it — the only way
 * to observe the in-flight reply while live events arrive. */
let deferStream = false;
let pendingStream: Array<(o: ChatStreamOutcome) => void> = [];

function nextOutcome(): ChatStreamOutcome {
  return outcomes.shift() ?? done("готово");
}

mock.module("../lib/llm", () => ({
  ...actualLlm,
  streamLlmChat: (...a: unknown[]) => {
    streamCalls.push(a);
    if (streamThrows) return Promise.reject(streamThrows);
    if (deferStream) {
      return new Promise<ChatStreamOutcome>((resolve) => pendingStream.push(resolve));
    }
    return Promise.resolve(nextOutcome());
  },
  streamLlmChatResume: async (...a: unknown[]) => {
    resumeCalls.push(a);
    return nextOutcome();
  },
  cancelLlmChat: async () => {
    cancelCalls += 1;
  },
  llmChatOnce: async () => ({ content: onceResponse, toolCalls: [], usage: null }),
  listenLlmChatDelta: async (cb: Listener<{ delta: string }>) => {
    deltaListeners.push(cb);
    return () => {
      deltaListeners = deltaListeners.filter((l) => l !== cb);
    };
  },
  listenLlmChatReasoningDelta: async (cb: Listener<{ delta: string }>) => {
    reasoningListeners.push(cb);
    return () => {
      reasoningListeners = reasoningListeners.filter((l) => l !== cb);
    };
  },
  listenLlmToolCall: async (cb: Listener<{ id: string; name: string; arguments: string }>) => {
    toolCallListeners.push(cb);
    return () => {
      toolCallListeners = toolCallListeners.filter((l) => l !== cb);
    };
  },
  listenLlmToolResult: async (cb: Listener<{ id: string; result: unknown; error: string | null }>) => {
    toolResultListeners.push(cb);
    return () => {
      toolResultListeners = toolResultListeners.filter((l) => l !== cb);
    };
  },
}));

mock.module("../lib/aiTools", () => ({
  ...actualAiTools,
  getAutoApprovedTools: async () => autoApprovedTools,
  setToolAutoApproved: async (tool: string, on: boolean) => {
    setAutoApprovedCalls.push([tool, on]);
  },
  onAutoApprovedToolsChange: () => () => {},
  getMemoryWake: async () => null,
}));

mock.module("../lib/assistantSounds", () => ({
  playNeedAnswerSound: () => {},
  playTaskDoneSound: () => {},
}));
mock.module("../lib/plans", () => ({ planGet: async () => null }));
mock.module("../lib/artifacts", () => ({ artifactList: async () => [] }));

const { useLlmChat } = await import("../hooks/useLlmChat");

// --- helpers ---------------------------------------------------------------

function done(text: string, over: Record<string, unknown> = {}): ChatStreamOutcome {
  return {
    status: "done",
    value: { text, reasoning: "", usage: null, todos: [], ...over },
  } as ChatStreamOutcome;
}

function cancelled(text = ""): ChatStreamOutcome {
  return {
    status: "cancelled",
    value: { text, reasoning: "", usage: null, todos: [] },
  } as ChatStreamOutcome;
}

function paused(calls: PendingToolCall[]): ChatStreamOutcome {
  return {
    status: "pendingApproval",
    value: { history: [], round: 1, budgetUsed: 1, calls, todos: [] } as PendingApproval,
  } as ChatStreamOutcome;
}

function call(id: string, name: string, requiresConfirmation = true): PendingToolCall {
  return { id, name, arguments: "{}", requiresConfirmation } as PendingToolCall;
}

type Callbacks = {
  onTurnSettled: ReturnType<typeof mock>;
  onTurnPaused: ReturnType<typeof mock>;
};

function render(
  over: {
    providerId?: string | null;
    initialMessages?: ChatMessage[];
    initialPendingResume?: PendingApproval | null;
  } = {},
) {
  const cbs: Callbacks = { onTurnSettled: mock(() => {}), onTurnPaused: mock(() => {}) };
  const hook = renderHook(() =>
    useLlmChat(
      over.providerId === undefined ? "openai" : over.providerId,
      null,
      "docsOnly" as never,
      "agent" as never,
      null,
      [],
      null,
      over.initialMessages ?? [],
      [],
      null,
      over.initialPendingResume ?? null,
      cbs.onTurnSettled,
      cbs.onTurnPaused,
      null,
      false,
      false,
    ),
  );
  return { ...hook, cbs };
}

async function emitDelta(text: string) {
  await act(async () => {
    for (const l of [...deltaListeners]) l({ delta: text });
  });
}

function lastAssistant(messages: ChatMessage[]) {
  return [...messages].reverse().find((m) => m.role === "assistant");
}

function textOf(m: ChatMessage | undefined) {
  return (m?.blocks ?? [])
    .filter((b) => b.type === "text")
    .map((b) => (b as { content: string }).content)
    .join("");
}

beforeEach(() => {
  deltaListeners = [];
  reasoningListeners = [];
  toolCallListeners = [];
  toolResultListeners = [];
  outcomes = [];
  streamThrows = null;
  streamCalls = [];
  resumeCalls = [];
  cancelCalls = 0;
  autoApprovedTools = [];
  setAutoApprovedCalls = [];
  onceResponse = "сводка";
  deferStream = false;
  pendingStream = [];
});

// --- tests -----------------------------------------------------------------

describe("useLlmChat — one plain turn", () => {
  test("sending adds the user turn and the model's reply", async () => {
    outcomes = [done("Ответ модели")];
    const { result } = render();

    await act(async () => {
      await result.current.sendMessage("вопрос");
    });

    expect(result.current.messages).toHaveLength(2);
    expect(result.current.messages[0]).toMatchObject({ role: "user", content: "вопрос" });
    expect(textOf(lastAssistant(result.current.messages))).toBe("Ответ модели");
    expect(result.current.sending).toBe(false);
  });

  test("the reply stops being marked as streaming once it settles", async () => {
    outcomes = [done("готово")];
    const { result } = render();
    await act(async () => {
      await result.current.sendMessage("вопрос");
    });
    expect(lastAssistant(result.current.messages)?.streaming).toBe(false);
  });

  test("the settled turn is reported once, for persistence", async () => {
    outcomes = [done("готово")];
    const { result, cbs } = render();
    await act(async () => {
      await result.current.sendMessage("вопрос");
    });
    expect(cbs.onTurnSettled).toHaveBeenCalledTimes(1);
  });

  test("blank input and a missing provider are both no-ops", async () => {
    const { result } = render({ providerId: null });
    await act(async () => {
      await result.current.sendMessage("вопрос");
    });
    expect(streamCalls).toHaveLength(0);

    const withProvider = render();
    await act(async () => {
      await withProvider.result.current.sendMessage("   ");
    });
    expect(streamCalls).toHaveLength(0);
  });
});

describe("useLlmChat — live events", () => {
  test("text deltas accumulate on the in-flight reply", async () => {
    deferStream = true;
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("вопрос");
      await Promise.resolve();
    });

    await emitDelta("Пишу");
    await emitDelta(" ответ");
    expect(textOf(lastAssistant(result.current.messages))).toBe("Пишу ответ");

    await act(async () => {
      pendingStream[0]?.(done("Пишу ответ"));
      await sent;
    });
  });

  test("a tool call becomes a permanent block, then settles in place", async () => {
    deferStream = true;
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("вопрос");
      await Promise.resolve();
    });

    await act(async () => {
      for (const l of [...toolCallListeners]) l({ id: "t1", name: "readFile", arguments: "{}" });
    });
    let block = lastAssistant(result.current.messages)?.blocks.find((b) => b.type === "toolCall");
    expect(block).toMatchObject({ name: "readFile", status: "running" });

    await act(async () => {
      for (const l of [...toolResultListeners]) l({ id: "t1", result: { ok: true }, error: null });
    });
    block = lastAssistant(result.current.messages)?.blocks.find((b) => b.type === "toolCall");
    // The block is settled in place, never removed — the transcript keeps a
    // record of what the assistant actually did.
    expect(block).toMatchObject({ name: "readFile" });
    expect((block as { status: string }).status).not.toBe("running");

    await act(async () => {
      pendingStream[0]?.(done(""));
      await sent;
    });
  });
});

describe("useLlmChat — tool approval", () => {
  test("a risky call pauses the turn and shows a card", async () => {
    outcomes = [paused([call("c1", "writeFile")]), done("записал")];
    const { result, cbs } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("запиши файл");
      await Promise.resolve();
    });

    await waitFor(() => expect(cbs.onTurnPaused).toHaveBeenCalled());
    // A pending card is a `toolCall` block in the `pendingApproval` state —
    // the same block that later transitions in place once it runs.
    const block = lastAssistant(result.current.messages)?.blocks.find(
      (b) => b.type === "toolCall" && (b as { status: string }).status === "pendingApproval",
    );
    expect(block).toMatchObject({ name: "writeFile" });
    // Nothing was resumed yet — the backend is waiting on the user.
    expect(resumeCalls).toHaveLength(0);

    await act(async () => {
      result.current.decideToolCall("c1", true, false);
      await sent;
    });

    expect(resumeCalls).toHaveLength(1);
    const decisions = resumeCalls[0]?.[4] as ToolCallDecision[];
    expect(decisions).toEqual([{ id: "c1", approved: true }]);
  });

  test("denying resumes with the refusal rather than cancelling the turn", async () => {
    outcomes = [paused([call("c1", "deleteFile")]), done("не стал удалять")];
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("удали");
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.sending).toBe(true));

    await act(async () => {
      result.current.decideToolCall("c1", false, false);
      await sent;
    });

    expect((resumeCalls[0]?.[4] as ToolCallDecision[])[0]).toMatchObject({ approved: false });
    expect(textOf(lastAssistant(result.current.messages))).toBe("не стал удалять");
  });

  test("a tool already trusted for this project never pauses", async () => {
    // The decision was made in an earlier chat and persisted; asking again
    // would defeat "Разрешать всегда".
    autoApprovedTools = ["writeFile"];
    outcomes = [paused([call("c1", "writeFile")]), done("записал")];
    const { result, cbs } = render();
    await waitFor(() => expect(deltaListeners.length).toBeGreaterThan(0));

    await act(async () => {
      await result.current.sendMessage("запиши");
    });

    expect(cbs.onTurnPaused).not.toHaveBeenCalled();
    expect((resumeCalls[0]?.[4] as ToolCallDecision[])[0]).toMatchObject({ approved: true });
  });

  test("approving with trust persists the choice for later chats", async () => {
    outcomes = [paused([call("c1", "writeFile")]), done("готово")];
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("запиши");
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.sending).toBe(true));

    await act(async () => {
      result.current.decideToolCall("c1", true, true);
      await sent;
    });

    expect(setAutoApprovedCalls).toEqual([["writeFile", true]]);
  });

  test("askUser always surfaces, even when trusted", async () => {
    // Trust must never skip a clarifying question — that would answer for
    // the user.
    autoApprovedTools = ["askUser"];
    outcomes = [paused([call("c1", "askUser")]), done("понял")];
    const { result, cbs } = render();
    await waitFor(() => expect(deltaListeners.length).toBeGreaterThan(0));

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("сделай");
      await Promise.resolve();
    });

    await waitFor(() => expect(cbs.onTurnPaused).toHaveBeenCalled());
    await act(async () => {
      result.current.answerAskUser("c1", { answers: ["да"] } as never);
      await sent;
    });

    const decisions = resumeCalls[0]?.[4] as ToolCallDecision[];
    expect(decisions[0]).toMatchObject({ id: "c1", approved: true });
    // Answering is not the same as trusting.
    expect(setAutoApprovedCalls).toEqual([]);
  });

  test("a batch pauses once and resumes with every decision", async () => {
    outcomes = [paused([call("c1", "writeFile"), call("c2", "deleteFile")]), done("готово")];
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("сделай");
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.sending).toBe(true));

    await act(async () => {
      result.current.decideToolCall("c1", true, false);
    });
    // One of two decided — still waiting.
    expect(resumeCalls).toHaveLength(0);

    await act(async () => {
      result.current.decideToolCall("c2", false, false);
      await sent;
    });

    const decisions = resumeCalls[0]?.[4] as ToolCallDecision[];
    expect(decisions).toHaveLength(2);
  });

  test("a non-risky call in the batch is not asked about", async () => {
    outcomes = [paused([call("c1", "readFile", false), call("c2", "writeFile")]), done("готово")];
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("сделай");
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.sending).toBe(true));

    const cards = lastAssistant(result.current.messages)?.blocks.filter(
      (b) => b.type === "toolCall" && (b as { status: string }).status === "pendingApproval",
    );
    expect(cards).toHaveLength(1);

    await act(async () => {
      result.current.decideToolCall("c2", true, false);
      await sent;
    });
  });
});

describe("useLlmChat — stopping", () => {
  test("stopping cancels the backend and unblocks a waiting card", async () => {
    outcomes = [paused([call("c1", "writeFile")]), cancelled("")];
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("сделай");
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.sending).toBe(true));

    await act(async () => {
      result.current.stopChat();
      await sent;
    });

    expect(cancelCalls).toBe(1);
    // Every pending call is denied so the loop can proceed to the resume
    // that then hits the backend's own cancel checkpoint.
    expect((resumeCalls[0]?.[4] as ToolCallDecision[])[0]).toMatchObject({ approved: false });
    expect(lastAssistant(result.current.messages)?.cancelled).toBe(true);
  });

  test("stopping with nothing in flight is harmless", async () => {
    const { result } = render();
    act(() => result.current.stopChat());
    expect(cancelCalls).toBe(1);
  });
});

describe("useLlmChat — failure and retry", () => {
  test("a failed turn is marked, not dropped", async () => {
    streamThrows = "provider unreachable";
    const { result, cbs } = render();

    await act(async () => {
      await result.current.sendMessage("вопрос");
    });

    const last = lastAssistant(result.current.messages);
    expect(last).toMatchObject({ failed: true, errorMessage: "provider unreachable" });
    expect(last?.streaming).toBe(false);
    // Still reported, so the failed turn is persisted like any other.
    expect(cbs.onTurnSettled).toHaveBeenCalledTimes(1);
  });

  test("a context-length failure is flagged so the retry action can appear", async () => {
    streamThrows = "This model's maximum context length is 128000 tokens";
    const { result } = render();

    await act(async () => {
      await result.current.sendMessage("вопрос");
    });

    expect(lastAssistant(result.current.messages)?.contextLengthExceeded).toBe(true);
  });

  test("retrying replaces the failed reply and resends the same question", async () => {
    streamThrows = "This model's maximum context length is 128000 tokens";
    const { result } = render();
    await act(async () => {
      await result.current.sendMessage("мой вопрос");
    });
    const failedId = lastAssistant(result.current.messages)!.id;

    streamThrows = null;
    outcomes = [done("вышло")];
    await act(async () => {
      result.current.retryWithCompaction(failedId);
      await new Promise((r) => setTimeout(r, 0));
    });

    await waitFor(() => expect(textOf(lastAssistant(result.current.messages))).toBe("вышло"));
    expect(result.current.messages.some((m) => m.id === failedId)).toBe(false);
    // The user's own turn is kept — they should not have to retype it.
    expect(result.current.messages.some((m) => m.role === "user" && m.content === "мой вопрос")).toBe(true);
  });

  test("retrying anything other than a failed reply is a no-op", async () => {
    outcomes = [done("готово")];
    const { result } = render();
    await act(async () => {
      await result.current.sendMessage("вопрос");
    });
    const okId = lastAssistant(result.current.messages)!.id;
    const before = streamCalls.length;

    act(() => result.current.retryWithCompaction(okId));
    expect(streamCalls).toHaveLength(before);
  });
});

describe("useLlmChat — resuming after a restart", () => {
  test("a chat saved mid-pause becomes answerable again", async () => {
    // The card itself is already in the restored transcript; what has to be
    // rebuilt is the ability to answer it.
    const restored: ChatMessage[] = [
      { id: "u1", role: "user", content: "запиши" },
      { id: "a1", role: "assistant", blocks: [], streaming: true },
    ];
    outcomes = [done("дописал")];
    const { result } = render({
      initialMessages: restored,
      initialPendingResume: {
        history: [],
        round: 2,
        budgetUsed: 3,
        calls: [call("c1", "writeFile")],
        todos: [],
      } as PendingApproval,
    });

    await waitFor(() => expect(result.current.sending).toBe(true));
    await act(async () => {
      result.current.decideToolCall("c1", true, false);
      await new Promise((r) => setTimeout(r, 0));
    });

    await waitFor(() => expect(resumeCalls).toHaveLength(1));
    // Resumed from exactly where it paused, not from round zero.
    expect(resumeCalls[0]?.[2]).toBe(2);
    expect(resumeCalls[0]?.[3]).toBe(3);
  });

  test("a restored chat whose last turn already settled is not resumed", async () => {
    // Resuming a settled turn would replay tool calls that already ran.
    const restored: ChatMessage[] = [
      { id: "u1", role: "user", content: "запиши" },
      { id: "a1", role: "assistant", blocks: [], streaming: false },
    ];
    const { result } = render({
      initialMessages: restored,
      initialPendingResume: {
        history: [],
        round: 2,
        budgetUsed: 3,
        calls: [call("c1", "writeFile")],
        todos: [],
      } as PendingApproval,
    });

    await act(async () => {
      await new Promise((r) => setTimeout(r, 10));
    });
    expect(result.current.sending).toBe(false);
    expect(resumeCalls).toHaveLength(0);
  });
});

describe("useLlmChat — requestArtifact pauses", () => {
  const pause = (name: string) =>
    ({
      status: "pendingApproval",
      value: { history: [], round: 1, budgetUsed: 1, calls: [call("a1", name)], todos: [] },
    }) as ChatStreamOutcome;

  test("answering carries the artifact id into the resume decision", async () => {
    outcomes = [pause("requestArtifact"), done("готово")];
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("документация на метод");
      await Promise.resolve();
    });

    await act(async () => {
      result.current.answerArtifact("a1", "artifact-42");
      await sent;
    });

    // The backend loads the record from the store by this id — the decision
    // deliberately does not carry the artifact's contents.
    const decisions = resumeCalls[0]![4] as Array<Record<string, unknown>>;
    expect(decisions).toEqual([{ id: "a1", approved: true, artifactId: "artifact-42" }]);
  });

  test("«заполню позже» resolves the pause without an artifact", async () => {
    outcomes = [pause("requestArtifact"), done("продолжаю без него")];
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("документация на метод");
      await Promise.resolve();
    });

    await act(async () => {
      result.current.decideToolCall("a1", false, false);
      await sent;
    });

    const decisions = resumeCalls[0]![4] as Array<Record<string, unknown>>;
    expect(decisions).toEqual([{ id: "a1", approved: false }]);
  });

  test("the card has no countdown — the user is filling a form in another tab", async () => {
    // An approval card auto-denies after TOOL_APPROVAL_TIMEOUT_MS. Doing
    // that to an artifact request would cancel work already in progress.
    outcomes = [pause("requestArtifact"), done("готово")];
    const { result } = render();

    await act(async () => {
      void result.current.sendMessage("документация на метод");
      await Promise.resolve();
    });

    const block = result.current.messages
      .flatMap((m) => (m.role === "assistant" ? m.blocks : []))
      .find((b) => b.type === "toolCall" && b.id === "a1");
    expect(block).toBeDefined();
    expect(block!.type === "toolCall" && block!.deadlineAt).toBeUndefined();
  });

  test("«разрешать всегда» never applies to it", async () => {
    // Trusting it would skip the very card that is the point of the tool.
    outcomes = [pause("requestArtifact"), done("готово")];
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("документация на метод");
      await Promise.resolve();
    });

    await act(async () => {
      result.current.decideToolCall("a1", true, true);
      await sent;
    });

    expect(setAutoApprovedCalls).toEqual([]);
  });

  test("an already-trusted tool name cannot skip the artifact card", async () => {
    // Even if `requestArtifact` somehow ended up in the persisted trust set,
    // the pause must still surface rather than silently auto-approving with
    // no artifact attached.
    autoApprovedTools = ["requestArtifact"];
    outcomes = [pause("requestArtifact"), done("готово")];
    const { result } = render();
    // The trust set loads asynchronously; without this the card could
    // surface simply because the list had not arrived yet.
    await waitFor(() => expect(deltaListeners.length).toBeGreaterThan(0));

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("документация на метод");
      await Promise.resolve();
    });

    await waitFor(() =>
      expect(
        result.current.messages
          .flatMap((m) => (m.role === "assistant" ? m.blocks : []))
          .some((b) => b.type === "toolCall" && b.status === "pendingApproval"),
      ).toBe(true),
    );

    await act(async () => {
      result.current.answerArtifact("a1", "artifact-7");
      await sent;
    });
  });
});

describe("useLlmChat — the todo checklist", () => {
  test("the model's final list replaces the local one", async () => {
    const tasks = [{ id: "t1", title: "Проверить", status: "pending" }];
    outcomes = [done("готово", { todos: tasks })];
    const { result } = render();

    await act(async () => {
      await result.current.sendMessage("вопрос");
    });

    expect(result.current.todos).toEqual(tasks as never);
  });

  test("a pause carries the list forward mid-turn", async () => {
    // The checklist can change in a round that then pauses; the panel must
    // show it while the user decides, not after.
    const mid = [{ id: "t1", title: "Шаг 1", status: "inProgress" }];
    outcomes = [
      { status: "pendingApproval", value: { history: [], round: 1, budgetUsed: 1, calls: [call("c1", "writeFile")], todos: mid } } as ChatStreamOutcome,
      done("готово", { todos: mid }),
    ];
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("сделай");
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.todos).toHaveLength(1));

    await act(async () => {
      result.current.decideToolCall("c1", true, false);
      await sent;
    });
  });

  test("clearing cancels the unfinished tasks and leaves the rest", async () => {
    // Cancelled, not deleted — the same status a model-driven update uses,
    // so the transcript still shows what was planned.
    const tasks = [
      { id: "t1", title: "Готово", status: "completed" },
      { id: "t2", title: "В работе", status: "inProgress" },
      { id: "t3", title: "Ждёт", status: "pending" },
    ];
    outcomes = [done("готово", { todos: tasks })];
    const { result, cbs } = render();
    await act(async () => {
      await result.current.sendMessage("вопрос");
    });

    act(() => result.current.clearTodos());

    expect(result.current.todos.map((t) => t.status)).toEqual([
      "completed",
      "cancelled",
      "cancelled",
    ]);
    // Persisted right away: a button pressed between turns has no other way
    // to survive a reload.
    expect(cbs.onTurnSettled).toHaveBeenCalledTimes(2);
  });
});
