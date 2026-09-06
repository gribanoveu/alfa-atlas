import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualLlm from "../lib/llm";
import * as actualAiTools from "../lib/aiTools";
import * as actualPlans from "../lib/plans";
import * as actualArtifacts from "../lib/artifacts";
import type { ChatMessage } from "../lib/chatBlocks";
import type {
  ChatStreamOutcome,
  ChatUsage,
  LlmTurnEvent,
  PendingApproval,
  PendingToolCall,
  ToolCallDecision,
} from "../lib/llm";

// --- backend doubles -------------------------------------------------------

/** Every turn event carries the turn that emitted it; the hook drops
 * anything stamped with a turn it is not currently running.
 *
 * There is one backend channel now (`llm:turn-event`), but the tests still
 * emit per kind: an assertion reads better as "a tool result arrives" than
 * as a hand-built envelope. The single `listenLlmTurnEvent` double below
 * registers one shim per kind into these arrays and folds them all back
 * into that one channel. */
type Listener<T> = (payload: T & { turnId: string }) => void;

/** What a test hands to `emit`. `turnId` is filled in by `emit` itself
 * unless the test overrides it to play a foreign or stale turn. */
type TestEmission<T> = T & { turnId?: string };
type ToolCallEmission = { id: string; name: string; arguments: string };
type ToolResultEmission = { id: string; result: unknown; error: string | null };

let deltaListeners: Listener<{ delta: string }>[] = [];
let reasoningListeners: Listener<{ delta: string }>[] = [];
let steeringListeners: Listener<{ id: string; text: string }>[] = [];
let toolCallDeltaListeners: Listener<ToolCallEmission>[] = [];
let toolCallListeners: Listener<ToolCallEmission>[] = [];
let toolResultListeners: Listener<ToolResultEmission>[] = [];
let contextUsageListeners: Listener<ChatUsage>[] = [];
let roundStartedListeners: Listener<Record<string, never>>[] = [];

/** The id the hook generated for the turn currently in flight — recorded
 * from whichever backend call it was handed to. Test emissions are stamped
 * with it, exactly as the real backend stamps its events. */
let lastTurnId = "";
let eventSeq = 0;
let eventRound = 1;

/** Stamps one event of the unified channel. `turnId` defaults to the live
 * turn, exactly as the backend stamps its own; a test that deliberately
 * emits a foreign or stale turn passes it explicitly, and it must survive
 * all the way to the hook — that is the whole point of those tests. */
function turnEvent(
  event: Omit<LlmTurnEvent, "turnId" | "seq" | "round" | "targetId">,
  targetId: string | null,
  turnId: string = lastTurnId,
): LlmTurnEvent {
  return {
    ...event,
    turnId,
    seq: ++eventSeq,
    round: eventRound,
    targetId,
  } as LlmTurnEvent;
}

/** Fires one backend event at every subscriber, stamped with the live turn
 * unless the payload names one itself. */
function emit<T>(listeners: Listener<T>[], payload: TestEmission<T>) {
  for (const l of [...listeners]) l({ turnId: lastTurnId, ...payload });
}

/** Outcomes handed back by `streamLlmChat`, then `streamLlmChatResume`. */
let outcomes: ChatStreamOutcome[] = [];
let streamThrows: string | null = null;
let streamCalls: unknown[][] = [];
let resumeCalls: unknown[][] = [];
let cancelCalls = 0;
let steerCalls: string[] = [];
let unsteerCalls: string[] = [];
/** Что бэкенд отвечает на отмену: `false` — заметку уже забрал раунд. */
let unsteerRemoves = true;
let autoApprovedTools: string[] = [];
let setAutoApprovedCalls: Array<[string, boolean]> = [];
let onceResponse = "сводка";
/** Same idea as `deferStream`, for the compaction summarizer — the only way
 * to observe the notice card while its pass is still in flight. */
let deferOnce = false;
let pendingOnce: Array<(r: { content: string | null }) => void> = [];
let onceThrows: string | null = null;
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
    lastTurnId = a[1] as string;
    eventSeq = 0;
    eventRound = 1;
    if (streamThrows) return Promise.reject(streamThrows);
    if (deferStream) {
      return new Promise<ChatStreamOutcome>((resolve) => pendingStream.push(resolve));
    }
    return Promise.resolve(nextOutcome());
  },
  streamLlmChatResume: async (...a: unknown[]) => {
    resumeCalls.push(a);
    lastTurnId = a[1] as string;
    return nextOutcome();
  },
  cancelLlmChat: async () => {
    cancelCalls += 1;
  },
  steerLlmChat: async (text: string) => {
    steerCalls.push(text);
    return `note-${steerCalls.length}`;
  },
  unsteerLlmChat: async (id: string) => {
    unsteerCalls.push(id);
    return unsteerRemoves;
  },
  llmChatOnce: () => {
    if (onceThrows) return Promise.reject(onceThrows);
    if (deferOnce) return new Promise((resolve) => pendingOnce.push(resolve));
    return Promise.resolve({ content: onceResponse, toolCalls: [], usage: null });
  },
  listenLlmTurnEvent: async (cb: (event: LlmTurnEvent) => void) => {
    const delta = ({ turnId, delta }: TestEmission<{ delta: string }>) =>
      cb(turnEvent({ type: "delta", payload: { delta } }, `round:${eventRound}:text`, turnId));
    const reasoning = ({ turnId, delta }: TestEmission<{ delta: string }>) =>
      cb(
        turnEvent(
          { type: "reasoning", payload: { delta } },
          `round:${eventRound}:reasoning`,
          turnId,
        ),
      );
    const steering = ({ turnId, id, text }: TestEmission<{ id: string; text: string }>) =>
      cb(turnEvent({ type: "steeringApplied", payload: { id, text } }, `steer:${id}`, turnId));
    const toolDelta = ({ turnId, ...payload }: TestEmission<ToolCallEmission>) =>
      cb(
        turnEvent(
          { type: "toolCallDelta", payload },
          `round:${eventRound}:tool:${payload.id}`,
          turnId,
        ),
      );
    const toolCall = ({ turnId, ...payload }: TestEmission<ToolCallEmission>) =>
      cb(
        turnEvent({ type: "toolCall", payload }, `round:${eventRound}:tool:${payload.id}`, turnId),
      );
    const toolResult = ({ turnId, ...payload }: TestEmission<ToolResultEmission>) =>
      cb(
        turnEvent(
          { type: "toolResult", payload } as never,
          `round:${eventRound}:tool:${payload.id}`,
          turnId,
        ),
      );
    const roundStarted = ({ turnId }: TestEmission<Record<string, never>>) => {
      eventRound += 1;
      cb(turnEvent({ type: "roundStarted" }, `round:${eventRound}`, turnId));
    };
    const usage = ({ turnId, ...payload }: TestEmission<ChatUsage>) =>
      cb(turnEvent({ type: "contextUsage", payload }, `round:${eventRound}`, turnId));

    deltaListeners.push(delta as never);
    reasoningListeners.push(reasoning as never);
    steeringListeners.push(steering as never);
    toolCallDeltaListeners.push(toolDelta as never);
    toolCallListeners.push(toolCall as never);
    toolResultListeners.push(toolResult as never);
    roundStartedListeners.push(roundStarted as never);
    contextUsageListeners.push(usage as never);
    return () => {
      deltaListeners = deltaListeners.filter((listener) => listener !== (delta as never));
      reasoningListeners = reasoningListeners.filter((listener) => listener !== (reasoning as never));
      steeringListeners = steeringListeners.filter((listener) => listener !== (steering as never));
      toolCallDeltaListeners = toolCallDeltaListeners.filter((listener) => listener !== (toolDelta as never));
      toolCallListeners = toolCallListeners.filter((listener) => listener !== (toolCall as never));
      toolResultListeners = toolResultListeners.filter((listener) => listener !== (toolResult as never));
      roundStartedListeners = roundStartedListeners.filter((listener) => listener !== (roundStarted as never));
      contextUsageListeners = contextUsageListeners.filter((listener) => listener !== (usage as never));
    };
  },
}));

let memoryWakeText = "";
let planRecordForGet: import("../lib/plans").PlanRecord | null = null;

mock.module("../lib/aiTools", () => ({
  ...actualAiTools,
  getAutoApprovedTools: async () => autoApprovedTools,
  setToolAutoApproved: async (tool: string, on: boolean) => {
    setAutoApprovedCalls.push([tool, on]);
  },
  onAutoApprovedToolsChange: () => () => {},
  getMemoryWake: async () => memoryWakeText,
}));

mock.module("../lib/assistantSounds", () => ({
  playNeedAnswerSound: () => {},
  playTaskDoneSound: () => {},
}));
// Both spread the real module rather than standing in for it wholesale:
// `mock.module` is global for the whole `bun test` run, so a replacement that
// drops the other exports is what every later test file importing them sees.
mock.module("../lib/plans", () => ({ ...actualPlans, planGet: async () => planRecordForGet }));
mock.module("../lib/artifacts", () => ({ ...actualArtifacts, artifactList: async () => [] }));

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
    contextLimit?: number | null;
    initialMessages?: ChatMessage[];
    initialPendingResume?: PendingApproval | null;
    initialActivePlanId?: string | null;
  } = {},
) {
  const cbs: Callbacks = { onTurnSettled: mock(() => {}), onTurnPaused: mock(() => {}) };
  const hook = renderHook(() =>
    useLlmChat(
      over.providerId === undefined ? "openai" : over.providerId,
      over.contextLimit ?? null,
      "docsOnly" as never,
      "agent" as never,
      null,
      [],
      null,
      over.initialMessages ?? [],
      [],
      over.initialActivePlanId ?? null,
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
    emit(deltaListeners, { delta: text });
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
  steeringListeners = [];
  toolCallDeltaListeners = [];
  toolCallListeners = [];
  toolResultListeners = [];
  contextUsageListeners = [];
  roundStartedListeners = [];
  lastTurnId = "";
  eventSeq = 0;
  eventRound = 1;
  outcomes = [];
  deferOnce = false;
  pendingOnce = [];
  onceThrows = null;
  streamThrows = null;
  streamCalls = [];
  resumeCalls = [];
  cancelCalls = 0;
  steerCalls = [];
  unsteerCalls = [];
  unsteerRemoves = true;
  autoApprovedTools = [];
  setAutoApprovedCalls = [];
  onceResponse = "сводка";
  deferStream = false;
  pendingStream = [];
  memoryWakeText = "";
  planRecordForGet = null;
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
      emit(toolCallListeners, { id: "t1", name: "readFile", arguments: "{}" });
    });
    let block = lastAssistant(result.current.messages)?.blocks.find((b) => b.type === "toolCall");
    expect(block).toMatchObject({ name: "readFile", status: "running" });

    await act(async () => {
      emit(toolResultListeners, { id: "t1", result: { ok: true }, error: null });
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

  test("a tool-call argument stream opens the block before execution starts", async () => {
    deferStream = true;
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("нарисуй схему");
      await Promise.resolve();
    });

    await act(async () => {
      emit(toolCallDeltaListeners, { id: "v1", name: "visualize", arguments: '{"title":"Оплата","source":"flow' });
    });
    let block = lastAssistant(result.current.messages)?.blocks.find((b) => b.type === "toolCall");
    expect(block).toMatchObject({
      name: "visualize",
      status: "running",
      argumentsJson: '{"title":"Оплата","source":"flow',
    });

    await act(async () => {
      emit(toolCallDeltaListeners, { id: "v1", name: "visualize", arguments: '{"title":"Оплата","source":"flowchart TD"}' });
    });
    block = lastAssistant(result.current.messages)?.blocks.find((b) => b.type === "toolCall");
    expect(block).toMatchObject({ argumentsJson: '{"title":"Оплата","source":"flowchart TD"}' });

    await act(async () => {
      emit(toolCallListeners, { id: "v1", name: "visualize", arguments: '{"title":"Оплата","source":"flowchart TD"}' });
    });
    const blocks = lastAssistant(result.current.messages)?.blocks.filter((b) => b.type === "toolCall") ?? [];
    expect(blocks).toHaveLength(1);

    await act(async () => {
      pendingStream[0]?.(done(""));
      await sent;
    });
  });

  test("a queued clarification can be taken back before the model sees it", async () => {
    deferStream = true;
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("вопрос");
      await Promise.resolve();
      await result.current.steerChat("первое");
      await result.current.steerChat("второе");
    });
    expect(result.current.pendingSteers.map((s) => s.text)).toEqual(["первое", "второе"]);

    await act(async () => {
      await result.current.unsteerChat("note-1");
    });

    expect(unsteerCalls).toEqual(["note-1"]);
    // Снимается ровно отменённое, соседнее уточнение остаётся в очереди.
    expect(result.current.pendingSteers).toEqual([{ id: "note-2", text: "второе" }]);

    await act(async () => {
      pendingStream[0]?.(done(""));
      await sent;
    });
  });

  test("a clarification the round already took stays visible until it is applied", async () => {
    deferStream = true;
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("вопрос");
      await Promise.resolve();
      await result.current.steerChat("не успели отменить");
    });

    // Бэкенд отвечает, что заметки в очереди уже нет — значит она ушла
    // модели, и убирать строку нельзя: пользователь должен увидеть, что
    // уточнение всё-таки применилось.
    unsteerRemoves = false;
    await act(async () => {
      await result.current.unsteerChat("note-1");
    });
    expect(result.current.pendingSteers).toEqual([
      { id: "note-1", text: "не успели отменить" },
    ]);

    await act(async () => {
      emit(steeringListeners, { id: "note-1", text: "не успели отменить" });
    });
    expect(result.current.pendingSteers).toEqual([]);

    await act(async () => {
      pendingStream[0]?.(done(""));
      await sent;
    });
  });

  test("applied steering becomes a permanent block and leaves the pending queue", async () => {
    deferStream = true;
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("вопрос");
      await Promise.resolve();
      await result.current.steerChat("Проверь ru locale");
    });
    expect(steerCalls).toEqual(["Проверь ru locale"]);
    expect(result.current.pendingSteers).toEqual([
      { id: "note-1", text: "Проверь ru locale" },
    ]);

    await act(async () => {
      emit(steeringListeners, { id: "note-1", text: "Проверь ru locale" });
    });

    expect(result.current.pendingSteers).toEqual([]);
    expect(lastAssistant(result.current.messages)?.blocks).toContainEqual(
      expect.objectContaining({ type: "steer", text: "Проверь ru locale" }),
    );

    await act(async () => {
      pendingStream[0]?.(done(""));
      await sent;
    });
  });
});

describe("useLlmChat — turn isolation", () => {
  test("a delta stamped with another turn is dropped", () => {
    // These events are global. Before the id existed, two overlapping turns
    // interleaved their tokens into one message, character by character.
    deferStream = true;
    return (async () => {
      const { result } = render();
      await act(async () => {
        void result.current.sendMessage("вопрос");
        await Promise.resolve();
      });
      await waitFor(() => expect(deltaListeners.length).toBeGreaterThan(0));

      await emitDelta("свой ход. ");
      await act(async () => {
        for (const l of [...deltaListeners]) l({ delta: "чужой ход", turnId: "someone-else" });
      });

      expect(textOf(lastAssistant(result.current.messages))).toBe("свой ход. ");

      await act(async () => {
        pendingStream.shift()?.(done("готово"));
      });
    })();
  });

  test("a straggler from a finished turn cannot write into the next message", async () => {
    outcomes = [done("первый ответ")];
    const { result } = render();
    await waitFor(() => expect(deltaListeners.length).toBeGreaterThan(0));

    await act(async () => {
      await result.current.sendMessage("первый вопрос");
    });
    const staleTurnId = lastTurnId;

    outcomes = [done("второй ответ")];
    deferStream = true;
    await act(async () => {
      void result.current.sendMessage("второй вопрос");
      await Promise.resolve();
    });

    await act(async () => {
      for (const l of [...deltaListeners]) l({ delta: "хвост первого хода", turnId: staleTurnId });
    });
    expect(textOf(lastAssistant(result.current.messages))).toBe("");

    await act(async () => {
      pendingStream.shift()?.(done("второй ответ"));
    });
  });

  test("a second send while a turn is in flight is refused", async () => {
    deferStream = true;
    const { result } = render();
    await act(async () => {
      void result.current.sendMessage("первый");
      await Promise.resolve();
    });

    await act(async () => {
      void result.current.sendMessage("второй");
      await Promise.resolve();
    });

    // One turn, one user message — no second bubble, no second stream.
    expect(streamCalls).toHaveLength(1);
    expect(result.current.messages.filter((m) => m.role === "user")).toHaveLength(1);

    await act(async () => {
      pendingStream.shift()?.(done("готово"));
    });
  });

  test("a round boundary closes the previous round's text block", async () => {
    // Two rounds of prose with no tool call between them (a steer, or an
    // app-authored note) used to be concatenated mid-sentence.
    deferStream = true;
    const { result } = render();
    await act(async () => {
      void result.current.sendMessage("вопрос");
      await Promise.resolve();
    });
    await waitFor(() => expect(roundStartedListeners.length).toBeGreaterThan(0));

    await emitDelta("итог — HTTP 200.");
    await act(async () => {
      emit(roundStartedListeners, {} as Record<string, never>);
    });
    await emitDelta("Теперь доступ к коду есть");

    const blocks = lastAssistant(result.current.messages)?.blocks ?? [];
    const texts = blocks.filter((b) => b.type === "text").map((b) => (b as { content: string }).content);
    expect(texts).toEqual(["итог — HTTP 200.", "Теперь доступ к коду есть"]);

    await act(async () => {
      pendingStream.shift()?.(done("готово"));
    });
  });

  test("the reply stops counting as thinking once the answer starts", async () => {
    // The reasoning block deliberately stays open (providers interleave), so
    // "still thinking" has to come from the last delta, not from the blocks.
    deferStream = true;
    const { result } = render();
    await act(async () => {
      void result.current.sendMessage("вопрос");
      await Promise.resolve();
    });
    await waitFor(() => expect(reasoningListeners.length).toBeGreaterThan(0));

    await act(async () => {
      emit(reasoningListeners, { delta: "размышляю" });
    });
    expect(lastAssistant(result.current.messages)?.liveKind).toBe("reasoning");

    await emitDelta("отвечаю");
    expect(lastAssistant(result.current.messages)?.liveKind).toBe("text");

    await act(async () => {
      pendingStream.shift()?.(done("готово"));
    });
    expect(lastAssistant(result.current.messages)?.liveKind).toBeUndefined();
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
    const decisions = resumeCalls[0]?.[5] as ToolCallDecision[];
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

    expect((resumeCalls[0]?.[5] as ToolCallDecision[])[0]).toMatchObject({ approved: false });
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
    expect((resumeCalls[0]?.[5] as ToolCallDecision[])[0]).toMatchObject({ approved: true });
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

    const decisions = resumeCalls[0]?.[5] as ToolCallDecision[];
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

    const decisions = resumeCalls[0]?.[5] as ToolCallDecision[];
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
    expect((resumeCalls[0]?.[5] as ToolCallDecision[])[0]).toMatchObject({ approved: false });
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
    expect(resumeCalls[0]?.[3]).toBe(2);
    expect(resumeCalls[0]?.[4]).toBe(3);
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
    const decisions = resumeCalls[0]![5] as Array<Record<string, unknown>>;
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

    const decisions = resumeCalls[0]![5] as Array<Record<string, unknown>>;
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

describe("useLlmChat — consent tools", () => {
  const pause = (name: string) =>
    ({
      status: "pendingApproval",
      value: { history: [], round: 1, budgetUsed: 1, calls: [call("c1", name)], todos: [] },
    }) as ChatStreamOutcome;

  for (const tool of ["requestModeSwitch", "requestFullRepoAccess"]) {
    test(`«разрешать всегда» never applies to ${tool}`, async () => {
      // These two decide what the assistant may do *next* — the read
      // boundary and the mode. Remembering the answer removes the only
      // checkpoint they have.
      outcomes = [pause(tool), done("готово")];
      const { result } = render();

      let sent!: Promise<void>;
      await act(async () => {
        sent = result.current.sendMessage("посмотри код");
        await Promise.resolve();
      });

      await act(async () => {
        result.current.decideToolCall("c1", true, true);
        await sent;
      });

      expect(setAutoApprovedCalls).toEqual([]);
      expect((resumeCalls[0]![5] as ToolCallDecision[])[0]).toMatchObject({ approved: true });
    });

    test(`${tool} has no auto-deny countdown`, async () => {
      // An expired countdown reaches the model as a refusal the user never
      // made — the exact failure this pair must never produce.
      outcomes = [pause(tool), done("готово")];
      const { result } = render();

      await act(async () => {
        void result.current.sendMessage("посмотри код");
        await Promise.resolve();
      });

      const block = result.current.messages
        .flatMap((m) => (m.role === "assistant" ? m.blocks : []))
        .find((b) => b.type === "toolCall" && b.id === "c1");
      expect(block!.type === "toolCall" && block!.deadlineAt).toBeUndefined();
    });

    test(`a persisted grant cannot skip the ${tool} card`, async () => {
      // Projects that ticked "Разрешать всегда" while that was still
      // possible keep the stale row; it must not silently auto-approve.
      autoApprovedTools = [tool];
      outcomes = [pause(tool), done("готово")];
      const { result, cbs } = render();
      await waitFor(() => expect(deltaListeners.length).toBeGreaterThan(0));

      let sent!: Promise<void>;
      await act(async () => {
        sent = result.current.sendMessage("посмотри код");
        await Promise.resolve();
      });

      await waitFor(() => expect(cbs.onTurnPaused).toHaveBeenCalled());
      await act(async () => {
        result.current.decideToolCall("c1", true, false);
        await sent;
      });
    });
  }
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

describe("useLlmChat — the context ring", () => {
  test("a tool result grows the estimate while the turn is still running", async () => {
    deferStream = true;
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("вопрос");
      await Promise.resolve();
    });

    await act(async () => {
      emit(toolCallListeners, { id: "t1", name: "readFile", arguments: "{}" });
    });
    const beforeResult = result.current.contextTokens;

    await act(async () => {
      emit(toolResultListeners, { id: "t1", result: { tool: "file", result: { content: "x".repeat(8000) } }, error: null });
    });

    // The backend keeps that payload in the turn's history and resends it on
    // every later round — the ring has to move, not wait for the turn to end.
    expect(result.current.contextTokens).toBeGreaterThan(beforeResult + 1500);

    await act(async () => {
      pendingStream[0]?.(done("готово"));
      await sent;
    });
  });

  test("a mid-turn usage report is a floor the estimate cannot undercut", async () => {
    deferStream = true;
    const { result } = render();

    let sent!: Promise<void>;
    await act(async () => {
      sent = result.current.sendMessage("вопрос");
      await Promise.resolve();
    });

    await act(async () => {
      emit(contextUsageListeners, { promptTokens: 890_000, completionTokens: 10_000, totalTokens: 900_000 });
    });
    expect(result.current.contextTokens).toBe(900_000);

    // Settled with no usage of its own: the provider's mid-turn number was
    // about *that* turn's rounds and must not linger as a floor afterwards.
    await act(async () => {
      pendingStream[0]?.(done("готово"));
      await sent;
    });
    expect(result.current.contextTokens).toBeLessThan(900_000);
  });
});

describe("useLlmChat — the compaction notice", () => {
  /** Long enough that `planCompaction` has something to fold: the retry path
   * keeps the last 6 real messages, so 8 leaves two to summarize. */
  function longHistory(): ChatMessage[] {
    return Array.from({ length: 8 }, (_, i) =>
      i % 2 === 0
        ? ({ id: `u${i}`, role: "user", content: `реплика ${i}` } as ChatMessage)
        : ({
            id: `a${i}`,
            role: "assistant",
            blocks: [{ type: "text", id: `b${i}`, content: `ответ ${i}` }],
            streaming: false,
          } as ChatMessage),
    );
  }

  /** Drives a turn to a context-length failure, then kicks off the retry —
   * the one path that compacts unconditionally, so the test doesn't have to
   * manufacture enough tokens to cross `shouldCompact`'s ratio. */
  async function failThenRetry(result: { current: ReturnType<typeof useLlmChat> }) {
    streamThrows = "This model's maximum context length is 128000 tokens";
    await act(async () => {
      await result.current.sendMessage("мой вопрос");
    });
    const failedId = lastAssistant(result.current.messages)!.id;
    streamThrows = null;
    outcomes = [done("вышло")];
    await act(async () => {
      result.current.retryWithCompaction(failedId);
      // Lets the pass reach its `await llmChatOnce` (and, when that call
      // isn't deferred, settle) inside `act` — the notice's insertion is a
      // state update, not a synchronous return value.
      await new Promise((r) => setTimeout(r, 0));
    });
  }

  function noticeIndex(messages: ChatMessage[]) {
    return messages.findIndex((m) => m.role === "assistant" && m.isCompactionNotice);
  }

  test("a card is shown while summarizing, above the user turn that triggered it", async () => {
    const { result } = render({ initialMessages: longHistory() });
    deferOnce = true;
    await failThenRetry(result);

    await waitFor(() => expect(noticeIndex(result.current.messages)).toBeGreaterThan(-1));
    const at = noticeIndex(result.current.messages);
    const notice = result.current.messages[at]!;
    expect(notice).toMatchObject({ compactionRunning: true, streaming: false });
    expect(textOf(notice)).toBe("Сжимаю историю…");
    // The point of the placement: it describes folding away *older* history,
    // so it sits above the question, not after the reply.
    expect(result.current.messages[at + 1]).toMatchObject({ role: "user", content: "мой вопрос" });

    await act(async () => {
      pendingOnce.shift()!({ content: "сводка" });
      await new Promise((r) => setTimeout(r, 0));
    });

    await waitFor(() => expect(result.current.messages[at]?.compactionRunning).toBeUndefined());
    // Settled in place — one event resolving, not a second message.
    expect(noticeIndex(result.current.messages)).toBe(at);
    expect(result.current.messages.filter((m) => m.role === "assistant" && m.isCompactionNotice)).toHaveLength(1);
    expect(textOf(result.current.messages[at])).toStartWith("История сжата");
  });

  test("a failed summarization leaves no notice behind", async () => {
    const { result } = render({ initialMessages: longHistory() });
    onceThrows = "provider unreachable";
    await failThenRetry(result);

    await waitFor(() => expect(textOf(lastAssistant(result.current.messages))).toBe("вышло"));
    expect(noticeIndex(result.current.messages)).toBe(-1);
  });

  test("an empty summary leaves no notice behind either", async () => {
    const { result } = render({ initialMessages: longHistory() });
    onceResponse = "   ";
    await failThenRetry(result);

    await waitFor(() => expect(textOf(lastAssistant(result.current.messages))).toBe("вышло"));
    expect(noticeIndex(result.current.messages)).toBe(-1);
  });
});

describe("useLlmChat — a loaded skill survives the turn that loaded it", () => {
  // Before the loaded-skills block existed, a `skill` result lived only in
  // the backend's per-turn history: `chatMessageToPlainText` drops tool
  // blocks and `toolLedger` has no arm for `skill`, so the next turn saw no
  // trace of it at all.
  test("re-injects the skill body as a system message on the next turn", async () => {
    const priorTurn: ChatMessage = {
      id: "a1",
      role: "assistant",
      streaming: false,
      blocks: [
        {
          type: "toolCall",
          id: "call_skill",
          name: "skill",
          argumentsJson: JSON.stringify({ op: "load", name: "jira-task-description" }),
          status: "done",
          result: {
            tool: "skillLoaded",
            result: {
              name: "jira-task-description",
              source: "bundled",
              body: "SKILL-BODY-MARKER",
              files: [],
            },
          },
        },
        { type: "text", id: "t1", content: "Готово." },
      ],
    };
    outcomes = [done("Ответ")];
    const { result } = render({
      initialMessages: [{ id: "u1", role: "user", content: "составь тикет" }, priorTurn],
    });

    await act(async () => {
      await result.current.sendMessage("а теперь добавь AC");
    });

    const wire = streamCalls[0]![2] as Array<{ role: string; content: string }>;
    const skillBlock = wire.find((m) => m.role === "system" && m.content.includes("[Skill]"));
    expect(skillBlock).toBeDefined();
    expect(skillBlock!.content).toContain("SKILL-BODY-MARKER");
    expect(skillBlock!.content).toContain("jira-task-description");
  });

  // The one tool result that cannot be fetched again: re-reading an
  // `askUser` answer means stopping and asking the person a second time.
  test("re-injects what the user answered to an earlier askUser", async () => {
    const priorTurn: ChatMessage = {
      id: "a1",
      role: "assistant",
      streaming: false,
      blocks: [
        {
          type: "toolCall",
          id: "call_ask",
          name: "askUser",
          argumentsJson: JSON.stringify({
            title: null,
            questions: [{ id: "q1", prompt: "Какой формат таблицы?", options: [], allowMultiple: false }],
          }),
          status: "done",
          result: {
            tool: "askUser",
            result: {
              answers: [
                {
                  questionId: "q1",
                  selectedOptionIds: ["o2"],
                  selectedLabels: ["Расширенный"],
                  customText: null,
                },
              ],
            },
          },
        },
      ],
    };
    outcomes = [done("Ответ")];
    const { result } = render({
      initialMessages: [{ id: "u1", role: "user", content: "опиши метод" }, priorTurn],
    });

    await act(async () => {
      await result.current.sendMessage("продолжай");
    });

    const wire = streamCalls[0]![2] as Array<{ role: string; content: string }>;
    const answers = wire.find((m) => m.role === "system" && m.content.includes("[Answers]"));
    expect(answers).toBeDefined();
    expect(answers!.content).toContain("Какой формат таблицы?");
    expect(answers!.content).toContain("Расширенный");
  });

  test("an ordinary conversation sends no skill block at all", async () => {
    outcomes = [done("Ответ")];
    const { result } = render();
    await act(async () => {
      await result.current.sendMessage("вопрос");
    });
    const wire = streamCalls[0]![2] as Array<{ role: string; content: string }>;
    expect(wire.some((m) => m.content.includes("[Skill]"))).toBe(false);
    expect(wire.some((m) => m.content.includes("[Answers]"))).toBe(false);
  });

  test("the context breakdown attributes the skill's tokens to it", async () => {
    const body = "z".repeat(8000);
    const { result } = render({
      initialMessages: [
        {
          id: "a1",
          role: "assistant",
          streaming: false,
          blocks: [
            {
              type: "toolCall",
              id: "call_skill",
              name: "skill",
              argumentsJson: JSON.stringify({ op: "load", name: "method-spec" }),
              status: "done",
              result: {
                tool: "skillLoaded",
                result: { name: "method-spec", source: "bundled", body, files: [] },
              },
            },
          ],
        },
      ],
    });

    const breakdown = result.current.contextBreakdown;
    expect(breakdown.skills).toBeGreaterThan(1500);
    expect(breakdown.total).toBe(
      breakdown.systemPrompt +
        breakdown.toolSchemas +
        breakdown.chat +
        breakdown.skills +
        breakdown.userAnswers +
        breakdown.plan +
        breakdown.memory,
    );
  });

  test("the context breakdown attributes askUser answers to their own bucket", () => {
    const { result } = render({
      initialMessages: [
        {
          id: "u1",
          role: "user",
          content: "опиши метод",
        },
        {
          id: "a1",
          role: "assistant",
          streaming: false,
          blocks: [
            {
              type: "toolCall",
              id: "call_ask",
              name: "askUser",
              argumentsJson: JSON.stringify({
                title: null,
                questions: [{ id: "q1", prompt: "Какой формат таблицы?", options: [], allowMultiple: false }],
              }),
              status: "done",
              result: {
                tool: "askUser",
                result: {
                  answers: [
                    {
                      questionId: "q1",
                      selectedOptionIds: ["o2"],
                      selectedLabels: ["Расширенный"],
                      customText: null,
                    },
                  ],
                },
              },
            },
          ],
        },
      ],
    });

    expect(result.current.contextBreakdown.userAnswers).toBeGreaterThan(0);
  });

  test("the context breakdown counts a live plan and memory wake", async () => {
    memoryWakeText = "z".repeat(4000);
    planRecordForGet = {
      id: "p1",
      name: "План",
      overview: "обзор",
      plan: "y".repeat(4000),
      todos: [],
      createdAtMs: 0,
      updatedAtMs: 0,
      chatId: null,
      repoRoot: null,
    };
    const { result } = render({ initialActivePlanId: "p1" });
    await waitFor(() => {
      expect(result.current.contextBreakdown.memory).toBeGreaterThan(500);
      expect(result.current.contextBreakdown.plan).toBeGreaterThan(500);
    });
  });
});
