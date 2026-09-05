import { describe, expect, test } from "bun:test";
import {
  acceptChatTurnEvent,
  chatTurnReducer,
  closeChatTurnProtocol,
  createChatTurnProtocol,
  normalizeChatTurnEvent,
  type ChatTurnProtocolState,
  type ChatTurnViewState,
} from "../lib/chatTurnReducer";
import type { ChatMessage, MessageBlock } from "../lib/chatBlocks";
import type { LlmTurnEvent } from "../lib/llm";

const TURN = "turn-1";

/** One event of `TURN`, with the fields the backend stamps. */
function event(
  seq: number,
  round: number,
  targetId: string | null,
  body: Omit<LlmTurnEvent, "turnId" | "seq" | "round" | "targetId">,
  turnId: string = TURN,
): LlmTurnEvent {
  return { turnId, seq, round, targetId, ...body } as LlmTurnEvent;
}

function delta(seq: number, round: number, text: string, turnId?: string): LlmTurnEvent {
  return event(seq, round, `round:${round}:text`, { type: "delta", payload: { delta: text } }, turnId);
}

/** A transcript mid-turn: one assistant message still streaming, which is
 * the only thing `updateLastAssistantBlocks` will write into. */
function streamingView(blocks: MessageBlock[] = []): ChatTurnViewState {
  const assistant: ChatMessage = {
    id: "assistant-1",
    role: "assistant",
    blocks,
    streaming: true,
  };
  return { messages: [assistant], retryState: null, liveUsage: null };
}

function blocksOf(state: ChatTurnViewState): MessageBlock[] {
  const last = state.messages[state.messages.length - 1]!;
  return last.role === "assistant" ? last.blocks : [];
}

function textOf(state: ChatTurnViewState): string {
  return blocksOf(state)
    .filter((block) => block.type === "text")
    .map((block) => (block.type === "text" ? block.content : ""))
    .join("|");
}

/** Feeds events through the protocol and folds the actions it releases,
 * exactly as `useLlmChat`'s single subscription does. */
function run(
  protocol: ChatTurnProtocolState,
  view: ChatTurnViewState,
  events: LlmTurnEvent[],
): { protocol: ChatTurnProtocolState; view: ChatTurnViewState } {
  let nextProtocol = protocol;
  let nextView = view;
  for (const one of events) {
    const accepted = acceptChatTurnEvent(nextProtocol, one);
    nextProtocol = accepted.state;
    nextView = accepted.actions.reduce(chatTurnReducer, nextView);
  }
  return { protocol: nextProtocol, view: nextView };
}

describe("chat turn protocol ordering", () => {
  test("an event delivered twice is applied once", () => {
    // The whole reason the envelope carries `seq`: a retried or duplicated
    // emit used to append the same tokens to the transcript a second time.
    const { view } = run(createChatTurnProtocol(TURN), streamingView(), [
      delta(1, 1, "Ответ"),
      delta(1, 1, "Ответ"),
    ]);
    expect(textOf(view)).toBe("Ответ");
  });

  test("events that arrive out of order are held back, then applied in order", () => {
    const first = run(createChatTurnProtocol(TURN), streamingView(), [
      delta(3, 1, "третий"),
      delta(2, 1, "второй"),
    ]);
    // Nothing may land while seq 1 is still missing: applying 2 and 3 now
    // would put the round's prose together in the wrong order for good.
    expect(textOf(first.view)).toBe("");

    const second = run(first.protocol, first.view, [delta(1, 1, "первый ")]);
    expect(textOf(second.view)).toBe("первый второйтретий");
  });

  test("an event from another turn is ignored", () => {
    const { view } = run(createChatTurnProtocol(TURN), streamingView(), [
      delta(1, 1, "свой ход"),
      delta(2, 1, "чужой ход", "turn-2"),
    ]);
    expect(textOf(view)).toBe("свой ход");
  });

  test("a straggler arriving after the turn closed is dropped", () => {
    const started = run(createChatTurnProtocol(TURN), streamingView(), [delta(1, 1, "ответ")]);
    const closed = closeChatTurnProtocol(started.protocol);

    const { view } = run(closed, started.view, [delta(2, 1, " хвост")]);
    expect(textOf(view)).toBe("ответ");
  });

  test("a resume continues the same sequence rather than restarting it", () => {
    // `PendingApproval.eventSeq` is the last seq the paused turn emitted;
    // the backend's next event is that plus one. Starting the protocol back
    // at 1 would buffer the whole resumed half of the turn forever.
    const resumed = createChatTurnProtocol(TURN, 8);
    const { view } = run(resumed, streamingView(), [delta(7, 2, "до паузы"), delta(8, 2, "после")]);
    expect(textOf(view)).toBe("после");
  });
});

describe("chat turn view reducer", () => {
  test("each round's prose goes to its own block, not the latest one", () => {
    const { view } = run(createChatTurnProtocol(TURN), streamingView(), [
      delta(1, 1, "Смотрю файл."),
      event(2, 1, "round:1:tool:t1", {
        type: "toolCall",
        payload: { id: "t1", name: "readFile", arguments: "{}" },
      }),
      event(3, 1, "round:1:tool:t1", {
        type: "toolResult",
        payload: { id: "t1", result: null, error: null },
      }),
      event(4, 2, "round:2", { type: "roundStarted" }),
      delta(5, 2, "Готово."),
    ]);
    expect(textOf(view)).toBe("Смотрю файл.|Готово.");
  });

  test("a round's authoritative text replaces the block its deltas built", () => {
    const { view } = run(createChatTurnProtocol(TURN), streamingView(), [
      delta(1, 1, "Отве"),
      event(2, 1, "round:1", {
        type: "roundCompleted",
        payload: { text: "Ответ целиком.", reasoning: "" },
      }),
    ]);
    expect(textOf(view)).toBe("Ответ целиком.");
  });

  test("the settled turn re-reports the final round without duplicating it", () => {
    // `roundCompleted` and the resolved IPC outcome both carry the final
    // round's text. Addressed by the same id, the second is an upsert.
    const streamed = run(createChatTurnProtocol(TURN), streamingView(), [
      delta(1, 1, "Ответ целиком."),
      event(2, 1, "round:1", {
        type: "roundCompleted",
        payload: { text: "Ответ целиком.", reasoning: "" },
      }),
    ]);

    const settled = chatTurnReducer(streamed.view, {
      type: "turnSettled",
      assistantId: "assistant-1",
      result: { text: "Ответ целиком.", reasoning: "", usage: null, truncated: false },
      stoppedByUser: false,
      durationMs: 1200,
      finalRound: streamed.protocol.lastRound,
    });

    expect(textOf(settled)).toBe("Ответ целиком.");
    const last = settled.messages[0]!;
    expect(last.role === "assistant" && last.streaming).toBe(false);
  });

  test("a steer applied twice does not produce two steer blocks", () => {
    const applied = event(1, 1, "steer:note-1", {
      type: "steeringApplied",
      payload: { id: "note-1", text: "Проверь ru locale" },
    });
    const state = [applied, applied].reduce(
      (view, one) => chatTurnReducer(view, normalizeChatTurnEvent(one)),
      streamingView(),
    );
    expect(blocksOf(state).filter((block) => block.type === "steer")).toHaveLength(1);
  });

  test("a retry notice clears as soon as the next attempt produces data", () => {
    const retrying = chatTurnReducer(
      streamingView(),
      normalizeChatTurnEvent(
        event(1, 1, "round:1", {
          type: "retrying",
          payload: { attempt: 1, maxAttempts: 3, delaySeconds: 10 },
        }),
      ),
    );
    expect(retrying.retryState).toEqual({ attempt: 1, maxAttempts: 3, delaySeconds: 10 });

    const recovered = chatTurnReducer(retrying, normalizeChatTurnEvent(delta(2, 1, "поехали")));
    expect(recovered.retryState).toBeNull();
  });

  test("a failed turn interrupts its running tool calls and keeps the reason", () => {
    const running = run(createChatTurnProtocol(TURN), streamingView(), [
      event(1, 1, "round:1:tool:t1", {
        type: "toolCall",
        payload: { id: "t1", name: "readFile", arguments: "{}" },
      }),
    ]);

    const failed = chatTurnReducer(running.view, {
      type: "turnFailed",
      assistantId: "assistant-1",
      errorMessage: "context length exceeded",
      contextLengthExceeded: true,
      durationMs: 40,
    });

    const message = failed.messages[0]!;
    expect(message.role === "assistant" && message.failed).toBe(true);
    expect(message.role === "assistant" && message.contextLengthExceeded).toBe(true);
    const tool = blocksOf(failed).find((block) => block.type === "toolCall");
    expect(tool?.type === "toolCall" && tool.status).toBe("error");
  });
});
