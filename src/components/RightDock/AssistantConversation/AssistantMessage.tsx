import { memo, useMemo } from "react";
import { AlertCircle, RefreshCw } from "lucide-react";
import { formatElapsedDuration } from "../../../hooks/useElapsedSeconds";
import {
  assistantAnswerText,
  groupBlocksForRender,
  isPlanToolBlock,
  isTicketToolBlock,
  isVisualToolBlock,
  lastBlockShowsLiveProgress,
  openStreamingBlockIds,
  type ChatMessage,
} from "../../../lib/chatBlocks";
import type { AskUserAnswerPayload } from "../../../lib/llm";
import type { Visual } from "../../../lib/visuals";
import { AssistantActivityGroup } from "../AssistantActivityGroup";
import { AssistantArtifactCard } from "../AssistantArtifactCard";
import { AssistantElapsedTimer } from "../AssistantElapsedTimer";
import { AssistantAskUserCard } from "../AssistantAskUserCard";
import { AssistantMarkdown } from "../AssistantMarkdown";
import { AssistantPlanCard } from "../AssistantPlanCard";
import {
  AssistantReasoningBlock,
  AssistantThinkingIndicator,
} from "../AssistantReasoningBlock";
import { AssistantSteerBlock } from "../AssistantSteerBlock";
import { AssistantTicketCard } from "../AssistantTicketCard";
import { AssistantToolApprovalGroup } from "../AssistantToolApprovalGroup";
import { AssistantToolCallBlock } from "../AssistantToolCallBlock";
import { AssistantUserMessage } from "../AssistantUserMessage";
import { AssistantVisualCard } from "../AssistantVisualCard";
import { CopyTextButton } from "../../Common/CopyTextButton";

const EMPTY_LIVE_BLOCK_IDS: ReadonlySet<string> = new Set<string>();

export type RetryState = {
  attempt: number;
  maxAttempts: number;
  delaySeconds: number;
} | null;

type AssistantMessageProps = {
  message: ChatMessage;
  chatId: string | null;
  docsRoot: string;
  repoRoot: string;
  sending: boolean;
  /** Only ever non-null for the message actually streaming — see the call
   * site in `AssistantTranscript`, which passes `null` for every other one so
   * a retry notice cannot re-render the whole settled transcript. */
  retryState: RetryState;
  onAnswerArtifact: (id: string, artifactId: string) => void;
  onAnswerAskUser: (id: string, answer: AskUserAnswerPayload) => void;
  onDecideToolCall: (id: string, approved: boolean, trust: boolean) => void;
  onRetryWithCompaction: (messageId: string) => void;
  onStartPlan: (planId: string) => void;
  onOpenPlan: (planId: string) => void;
  onOpenArtifact: (artifactId: string) => void;
  onOpenVisual: (visual: Visual) => void;
  onVisualRenderError: (note: string) => void;
  onRedrawVisual: (request: string) => void;
};

/** One bubble in the transcript.
 *
 * Its own component, and memoized, because the backend emits one event per
 * streamed token: without this, a single delta re-rendered every message in
 * the conversation — re-grouping their blocks and re-parsing their Markdown —
 * to change the last few characters of the last one. `updateLastAssistantBlocks`
 * rebuilds only the trailing message and `slice`s the rest through untouched,
 * so past messages arrive here with an unchanged reference and the default
 * shallow comparison skips them.
 *
 * That only holds while every callback prop stays referentially stable: an
 * arrow written inline at the call site would compare unequal on every token
 * and undo all of it. See `AssistantConversation`'s `handleOpenPlan` and
 * friends, and `useLlmChat`'s `messagesRef`, which is what lets `sendMessage`
 * and `retryWithCompaction` survive a delta. */
export const AssistantMessage = memo(function AssistantMessage({
  message,
  chatId,
  docsRoot,
  repoRoot,
  sending,
  retryState,
  onAnswerArtifact,
  onAnswerAskUser,
  onDecideToolCall,
  onRetryWithCompaction,
  onStartPlan,
  onOpenPlan,
  onOpenArtifact,
  onOpenVisual,
  onVisualRenderError,
  onRedrawVisual,
}: AssistantMessageProps) {
  const blocks = message.role === "assistant" ? message.blocks : null;
  const rendered = useMemo(
    () => (blocks ? groupBlocksForRender(blocks) : []),
    [blocks],
  );

  if (message.role !== "assistant") {
    return (
      <div className="assistant-chat-message user">
        <AssistantUserMessage content={message.content} />
      </div>
    );
  }

  const failed = Boolean(message.failed);
  const stopped = Boolean(message.cancelled);
  const truncated = Boolean(message.truncated);
  const liveBlockIds = message.streaming
    ? openStreamingBlockIds(message.blocks)
    : EMPTY_LIVE_BLOCK_IDS;
  const liveKind = message.liveKind;
  const answerText = !message.streaming ? assistantAnswerText(message.blocks) : "";
  // A trailing activity group speaks for the whole in-flight run, including
  // the silent gap between two tool calls — so the standalone thinking card
  // below would be a second live line saying the same thing.
  const liveActivityTail =
    message.streaming === true &&
    rendered[rendered.length - 1]?.kind === "activityGroup";

  return (
    <div
      className={`assistant-chat-message ${message.role}${failed ? " failed" : ""}${
        stopped ? " cancelled" : ""
      }`}
    >
      {message.blocks.length === 0 && message.streaming ? (
        <AssistantThinkingIndicator />
      ) : (
        <div className="assistant-chat-blocks">
          {rendered.map((item, index) =>
            item.kind === "activityGroup" ? (
              <AssistantActivityGroup
                key={item.blocks[0]!.id}
                blocks={item.blocks}
                active={liveActivityTail && index === rendered.length - 1}
                liveBlockIds={liveBlockIds}
                liveKind={liveKind}
              />
            ) : item.kind === "artifactGroup" ? (
              <AssistantArtifactCard
                key={item.blocks[0]!.id}
                blocks={item.blocks}
                chatId={chatId}
                onAnswer={onAnswerArtifact}
                onDefer={(id) => onDecideToolCall(id, false, false)}
              />
            ) : item.kind === "askGroup" ? (
              <AssistantAskUserCard
                key={item.blocks[0]!.id}
                blocks={item.blocks}
                onAnswer={onAnswerAskUser}
                onSkip={(id) => onDecideToolCall(id, false, false)}
              />
            ) : item.kind === "approvalGroup" ? (
              <AssistantToolApprovalGroup
                key={item.blocks[0]!.id}
                blocks={item.blocks}
                docsRoot={docsRoot}
                repoRoot={repoRoot}
                onDecide={onDecideToolCall}
              />
            ) : item.block.type === "reasoning" ? (
              <AssistantReasoningBlock
                key={item.block.id}
                block={item.block}
                thinking={
                  liveBlockIds.has(item.block.id) && liveKind === "reasoning"
                }
              />
            ) : item.block.type === "text" ? (
              <AssistantMarkdown
                key={item.block.id}
                content={item.block.content}
                streaming={
                  liveBlockIds.has(item.block.id) && liveKind !== "reasoning"
                }
              />
            ) : item.block.type === "steer" ? (
              <AssistantSteerBlock key={item.block.id} block={item.block} />
            ) : isPlanToolBlock(item.block) ? (
              <AssistantPlanCard
                key={item.block.id}
                block={item.block}
                startDisabled={sending}
                onOpenPlan={onOpenPlan}
                onStartPlan={onStartPlan}
              />
            ) : isTicketToolBlock(item.block) ? (
              <AssistantTicketCard
                key={item.block.id}
                block={item.block}
                onOpenArtifact={onOpenArtifact}
              />
            ) : isVisualToolBlock(item.block) ? (
              <AssistantVisualCard
                key={item.block.id}
                block={item.block}
                turnActive={message.streaming === true}
                onOpenVisual={onOpenVisual}
                onRenderError={onVisualRenderError}
                onRedraw={onRedrawVisual}
              />
            ) : (
              <AssistantToolCallBlock key={item.block.id} block={item.block} />
            ),
          )}
          {message.streaming &&
          !liveActivityTail &&
          !lastBlockShowsLiveProgress(message.blocks) ? (
            <AssistantThinkingIndicator />
          ) : null}
          {message.streaming && retryState ? (
            <div
              className="assistant-chat-retry-status"
              role="status"
              aria-live="polite"
            >
              <RefreshCw
                className="assistant-chat-retry-icon"
                size={13}
                strokeWidth={1.75}
                aria-hidden
              />
              <span>
                Связь прервана. Повторная попытка {retryState.attempt}/
                {retryState.maxAttempts} через {retryState.delaySeconds} с.
              </span>
            </div>
          ) : null}
          {stopped ? (
            <div className="assistant-chat-cancelled-note">
              Остановлено пользователем
            </div>
          ) : null}
          {truncated ? (
            <div className="assistant-chat-truncated-note">
              <AlertCircle size={13} aria-hidden />
              <span>
                Ответ обрезан: исчерпан лимит длины ответа. Увеличьте «Лимит
                ответа» в настройках провайдера или попросите продолжить.
              </span>
            </div>
          ) : null}
          {failed ? (
            <div className="assistant-chat-error-card">
              <AlertCircle size={13} aria-hidden />
              <span>{message.errorMessage ?? "Не удалось получить ответ"}</span>
              {message.contextLengthExceeded ? (
                <button
                  type="button"
                  className="assistant-chat-error-retry"
                  onClick={() => onRetryWithCompaction(message.id)}
                >
                  Сжать историю и повторить
                </button>
              ) : null}
            </div>
          ) : null}
          {!message.streaming &&
          (typeof message.durationMs === "number" || answerText !== "") ? (
            <div className="assistant-chat-answer-footer">
              {typeof message.durationMs === "number" ? (
                <span className="assistant-chat-duration">
                  Готово за{" "}
                  {formatElapsedDuration(Math.round(message.durationMs / 1000))}
                </span>
              ) : null}
              {answerText !== "" ? (
                <CopyTextButton
                  text={answerText}
                  className="assistant-chat-answer-copy"
                  label="Копировать ответ"
                />
              ) : null}
            </div>
          ) : null}
        </div>
      )}
      {/* Whole-turn timer. Sits outside the ternary above so it keeps its
          mount instant (and its count) when the bubble switches from the
          empty-thinking branch to the blocks branch; unmounts — and so
          disappears — the moment streaming ends, leaving the settled
          "Готово за …" line as the only duration. */}
      {message.streaming ? (
        <div className="assistant-chat-answer-footer">
          <AssistantElapsedTimer running className="assistant-chat-duration" />
        </div>
      ) : null}
    </div>
  );
});
