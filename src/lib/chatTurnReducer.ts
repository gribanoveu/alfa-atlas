import {
  appendDeltaToBlocks,
  appendReasoningDeltaToBlocks,
  appendSteerBlock,
  appendToolCallBlock,
  applyToolCallDelta,
  closeOpenBlocks,
  correctRoundText,
  correctTrailingReasoning,
  markRunningToolCallsAsInterrupted,
  settleToolCallBlock,
  updateLastAssistantBlocks,
  type ChatMessage,
} from "./chatBlocks";
import type { ToolResult } from "./aiTools";
import type { ChatStreamResult, ChatUsage, LlmTurnEvent } from "./llm";

export type ChatRetryState = {
  attempt: number;
  maxAttempts: number;
  delaySeconds: number;
};

export type ChatTurnViewState = {
  messages: ChatMessage[];
  retryState: ChatRetryState | null;
  liveUsage: ChatUsage | null;
};

type EventActionBase = {
  round: number;
  targetId: string | null;
};

export type ChatTurnAction =
  | (EventActionBase & { type: "delta"; delta: string })
  | (EventActionBase & { type: "reasoning"; delta: string })
  | (EventActionBase & { type: "retrying"; retry: ChatRetryState })
  | (EventActionBase & { type: "roundStarted" })
  | (EventActionBase & { type: "roundCompleted"; text: string; reasoning: string })
  | (EventActionBase & { type: "steeringApplied"; id: string; text: string })
  | (EventActionBase & {
      type: "toolCallDelta" | "toolCall";
      id: string;
      name: string;
      argumentsJson: string;
      autoApproved?: boolean;
    })
  | (EventActionBase & {
      type: "toolResult";
      id: string;
      result: ToolResult | null;
      error: string | null;
    })
  | (EventActionBase & { type: "contextUsage"; usage: ChatUsage })
  | (EventActionBase & { type: "rateLimitChanged" })
  | {
      type: "turnSettled";
      assistantId: string;
      result: ChatStreamResult;
      stoppedByUser: boolean;
      durationMs: number;
      finalRound: number;
    }
  | {
      type: "turnFailed";
      assistantId: string;
      errorMessage: string;
      contextLengthExceeded: boolean;
      durationMs: number;
    };

export type ChatTurnReducer = (
  state: ChatTurnViewState,
  action: ChatTurnAction,
) => ChatTurnViewState;

function roundTarget(round: number, kind: "text" | "reasoning"): string {
  return `round:${round}:${kind}`;
}

/** Pure transcript/view transition for normalized chat-turn actions. */
export const chatTurnReducer: ChatTurnReducer = (state, action) => {
  if (action.type === "retrying") {
    return { ...state, retryState: action.retry };
  }
  if (action.type === "contextUsage") {
    return { ...state, liveUsage: action.usage };
  }
  if (action.type === "rateLimitChanged") return state;

  if (action.type === "turnSettled") {
    const { text, reasoning, usage, truncated } = action.result;
    return {
      ...state,
      retryState: null,
      messages: state.messages.map((message) => {
        if (message.id !== action.assistantId || message.role !== "assistant") return message;
        const reconciled = correctRoundText(
          correctTrailingReasoning(
            message.blocks,
            reasoning ?? "",
            roundTarget(action.finalRound, "reasoning"),
          ),
          text,
          roundTarget(action.finalRound, "text"),
        );
        return {
          ...message,
          blocks: action.stoppedByUser
            ? markRunningToolCallsAsInterrupted(reconciled, "Остановлено пользователем")
            : reconciled,
          streaming: false,
          liveKind: undefined,
          usage: usage ?? undefined,
          cancelled: action.stoppedByUser,
          truncated: truncated === true,
          durationMs: action.durationMs,
        };
      }),
    };
  }

  if (action.type === "turnFailed") {
    return {
      ...state,
      retryState: null,
      messages: state.messages.map((message) =>
        message.id === action.assistantId && message.role === "assistant"
          ? {
              ...message,
              blocks: markRunningToolCallsAsInterrupted(message.blocks),
              streaming: false,
              failed: true,
              errorMessage: action.errorMessage,
              contextLengthExceeded: action.contextLengthExceeded,
              durationMs: action.durationMs,
            }
          : message,
      ),
    };
  }

  const update = (updater: Parameters<typeof updateLastAssistantBlocks>[1], liveKind?: "text" | "reasoning") => ({
    ...state,
    retryState:
      action.type === "delta" ||
      action.type === "reasoning" ||
      action.type === "roundCompleted" ||
      action.type === "toolCallDelta" ||
      action.type === "toolCall"
        ? null
        : state.retryState,
    messages: updateLastAssistantBlocks(state.messages, updater, liveKind),
  });

  switch (action.type) {
    case "delta":
      return update(
        (blocks) => appendDeltaToBlocks(blocks, action.delta, action.targetId ?? undefined),
        "text",
      );
    case "reasoning":
      return update(
        (blocks) => appendReasoningDeltaToBlocks(blocks, action.delta, action.targetId ?? undefined),
        "reasoning",
      );
    case "roundStarted":
      return update(closeOpenBlocks);
    case "roundCompleted":
      return update((blocks) =>
        correctRoundText(
          correctTrailingReasoning(
            blocks,
            action.reasoning,
            roundTarget(action.round, "reasoning"),
          ),
          action.text,
          roundTarget(action.round, "text"),
        ),
      );
    case "steeringApplied":
      return update((blocks) =>
        appendSteerBlock(blocks, action.text, action.targetId ?? `steer:${action.id}`),
      );
    case "toolCallDelta":
      return update((blocks) =>
        applyToolCallDelta(blocks, {
          id: action.id,
          name: action.name,
          argumentsJson: action.argumentsJson,
        }),
      );
    case "toolCall":
      return update((blocks) =>
        appendToolCallBlock(blocks, {
          id: action.id,
          name: action.name,
          argumentsJson: action.argumentsJson,
          autoApproved: action.autoApproved,
        }),
      );
    case "toolResult":
      return update((blocks) =>
        settleToolCallBlock(blocks, {
          id: action.id,
          result: action.result,
          error: action.error,
        }),
      );
  }
};

export type ChatTurnProtocolState = {
  turnId: string;
  nextSeq: number;
  lastRound: number;
  closed: boolean;
  buffered: ReadonlyMap<number, LlmTurnEvent>;
};

export function createChatTurnProtocol(turnId: string, nextSeq = 1): ChatTurnProtocolState {
  return { turnId, nextSeq, lastRound: 0, closed: false, buffered: new Map() };
}

export function closeChatTurnProtocol(state: ChatTurnProtocolState): ChatTurnProtocolState {
  return state.closed ? state : { ...state, closed: true, buffered: new Map() };
}

export function normalizeChatTurnEvent(event: LlmTurnEvent): ChatTurnAction {
  const base = { round: event.round, targetId: event.targetId };
  switch (event.type) {
    case "delta":
      return { ...base, type: "delta", delta: event.payload.delta };
    case "reasoning":
      return { ...base, type: "reasoning", delta: event.payload.delta };
    case "retrying":
      return { ...base, type: "retrying", retry: event.payload };
    case "roundStarted":
      return { ...base, type: "roundStarted" };
    case "roundCompleted":
      return { ...base, type: "roundCompleted", ...event.payload };
    case "steeringApplied":
      return { ...base, type: "steeringApplied", ...event.payload };
    case "toolCallDelta":
    case "toolCall":
      return {
        ...base,
        type: event.type,
        id: event.payload.id,
        name: event.payload.name,
        argumentsJson: event.payload.arguments,
      };
    case "toolResult":
      return { ...base, type: "toolResult", ...event.payload };
    case "contextUsage":
      return { ...base, type: "contextUsage", usage: event.payload };
    case "rateLimitChanged":
      return { ...base, type: "rateLimitChanged" };
  }
}

/**
 * Orders the unified channel before actions reach the view reducer.
 * Duplicate/late events are ignored; future events are buffered until every
 * preceding sequence arrives. Closing a turn makes all stragglers no-ops.
 */
export function acceptChatTurnEvent(
  state: ChatTurnProtocolState,
  event: LlmTurnEvent,
): { state: ChatTurnProtocolState; actions: ChatTurnAction[] } {
  if (state.closed || event.turnId !== state.turnId || event.seq < state.nextSeq) {
    return { state, actions: [] };
  }

  const buffered = new Map(state.buffered);
  if (!buffered.has(event.seq)) buffered.set(event.seq, event);

  const actions: ChatTurnAction[] = [];
  let nextSeq = state.nextSeq;
  let lastRound = state.lastRound;
  while (buffered.has(nextSeq)) {
    const next = buffered.get(nextSeq)!;
    buffered.delete(nextSeq);
    actions.push(normalizeChatTurnEvent(next));
    lastRound = next.round;
    nextSeq += 1;
  }

  return {
    state: { ...state, nextSeq, lastRound, buffered },
    actions,
  };
}
