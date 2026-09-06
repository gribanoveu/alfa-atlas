import type {
  RefObject,
  UIEventHandler,
  WheelEventHandler,
} from "react";
import {
  AlertCircle,
  ArrowDown,
  RefreshCw,
  Sparkles,
} from "lucide-react";
import { formatElapsedDuration } from "../../../hooks/useElapsedSeconds";
import type { AssistantSuggestion } from "../../../lib/assistantSuggestions";
import type { ConversationMode } from "../../../lib/aiTools";
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
import { AssistantAskUserCard } from "../AssistantAskUserCard";
import { AssistantCompactionNotice } from "../AssistantCompactionNotice";
import { AssistantMarkdown } from "../AssistantMarkdown";
import { AssistantPlanCard } from "../AssistantPlanCard";
import {
  AssistantReasoningBlock,
  AssistantThinkingIndicator,
} from "../AssistantReasoningBlock";
import { AssistantSteerBlock } from "../AssistantSteerBlock";
import { AssistantSuggestionChip } from "../AssistantSuggestionChip";
import { AssistantTicketCard } from "../AssistantTicketCard";
import { AssistantToolApprovalGroup } from "../AssistantToolApprovalGroup";
import { AssistantToolCallBlock } from "../AssistantToolCallBlock";
import { AssistantUserMessage } from "../AssistantUserMessage";
import { AssistantVisualCard } from "../AssistantVisualCard";
import { CopyTextButton } from "../CopyTextButton";
import { CHAT_MODE_OPTIONS } from "./AssistantModelControls";

const EMPTY_LIVE_BLOCK_IDS: ReadonlySet<string> = new Set<string>();

type RetryState = {
  attempt: number;
  maxAttempts: number;
  delaySeconds: number;
} | null;

type SuggestionGroup = {
  mode: ConversationMode;
  items: AssistantSuggestion[];
};

type AssistantTranscriptProps = {
  chatId: string | null;
  messages: ChatMessage[];
  sending: boolean;
  retryState: RetryState;
  conversationMode: ConversationMode;
  suggestionsByMode: SuggestionGroup[];
  hasAnySuggestion: boolean;
  hoveredSuggestion: AssistantSuggestion | null;
  docsRoot: string;
  repoRoot: string;
  scrollRef: RefObject<HTMLDivElement | null>;
  showJumpToBottom: boolean;
  onScroll: UIEventHandler<HTMLDivElement>;
  onWheel: WheelEventHandler<HTMLDivElement>;
  onJumpToBottom: () => void;
  onConversationModeChange: (mode: ConversationMode) => void;
  onSuggestionClick: (suggestion: AssistantSuggestion) => void;
  onSuggestionHoverChange: (suggestion: AssistantSuggestion | null) => void;
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

export function AssistantTranscript({
  chatId,
  messages,
  sending,
  retryState,
  conversationMode,
  suggestionsByMode,
  hasAnySuggestion,
  hoveredSuggestion,
  docsRoot,
  repoRoot,
  scrollRef,
  showJumpToBottom,
  onScroll,
  onWheel,
  onJumpToBottom,
  onConversationModeChange,
  onSuggestionClick,
  onSuggestionHoverChange,
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
}: AssistantTranscriptProps) {
  return (
    <div className="assistant-chat-scroll-shell">
      <div
        className="assistant-chat-messages"
        ref={scrollRef}
        onScroll={onScroll}
        onWheel={onWheel}
      >
        {messages.length === 0 ? (
          <div className="assistant-chat-placeholder">
            <Sparkles
              className="assistant-chat-placeholder-icon"
              size={20}
              strokeWidth={1.5}
              aria-hidden
            />
            <p className="assistant-chat-placeholder-title">Привет! Я Атлас</p>
            <div className="assistant-chat-stack is-inline">
              {CHAT_MODE_OPTIONS.map((option) => (
                <p
                  key={option.value}
                  className="assistant-chat-placeholder-desc"
                  data-inactive={
                    option.value === conversationMode ? undefined : "true"
                  }
                  aria-hidden={
                    option.value === conversationMode ? undefined : true
                  }
                >
                  {option.greeting}
                </p>
              ))}
            </div>

            <div
              className="assistant-chat-mode-chips"
              role="group"
              aria-label="Режим ассистента"
            >
              {CHAT_MODE_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  className={`assistant-chat-mode-chip${
                    conversationMode === option.value ? " is-active" : ""
                  }`}
                  title={option.title}
                  aria-pressed={conversationMode === option.value}
                  disabled={sending}
                  onClick={() => onConversationModeChange(option.value)}
                >
                  {option.label}
                </button>
              ))}
            </div>

            {hasAnySuggestion ? (
              <div className="assistant-chat-placeholder-suggestions">
                <p className="assistant-chat-placeholder-suggestions-label">
                  Шаблоны задач
                </p>
                <div className="assistant-chat-stack">
                  {suggestionsByMode.map(({ mode, items }) => (
                    <div
                      key={mode}
                      className="assistant-chat-suggestions"
                      data-inactive={
                        mode === conversationMode ? undefined : "true"
                      }
                      aria-hidden={
                        mode === conversationMode ? undefined : true
                      }
                    >
                      {items.map((suggestion) => (
                        <AssistantSuggestionChip
                          key={suggestion.id}
                          suggestion={suggestion}
                          className="assistant-suggestion-chip"
                          onClick={() => onSuggestionClick(suggestion)}
                          onHoverChange={onSuggestionHoverChange}
                        />
                      ))}
                    </div>
                  ))}
                </div>
                <p
                  className="assistant-chat-suggestion-desc"
                  aria-live="polite"
                >
                  {hoveredSuggestion?.hint ?? "\u00a0"}
                </p>
              </div>
            ) : null}
          </div>
        ) : (
          messages.map((message) => {
            if (
              message.role === "assistant" &&
              message.isCompactionNotice
            ) {
              return (
                <AssistantCompactionNotice
                  key={message.id}
                  message={message}
                />
              );
            }
            const failed =
              message.role === "assistant" && Boolean(message.failed);
            const stopped =
              message.role === "assistant" && Boolean(message.cancelled);
            const truncated =
              message.role === "assistant" && Boolean(message.truncated);
            const liveBlockIds =
              message.role === "assistant" && message.streaming
                ? openStreamingBlockIds(message.blocks)
                : EMPTY_LIVE_BLOCK_IDS;
            const liveKind =
              message.role === "assistant" ? message.liveKind : undefined;
            const rendered =
              message.role === "assistant"
                ? groupBlocksForRender(message.blocks)
                : [];
            // A trailing activity group speaks for the whole in-flight run,
            // including the silent gap between two tool calls — so the
            // standalone thinking card below would be a second live line
            // saying the same thing.
            const answerText =
              message.role === "assistant" && !message.streaming
                ? assistantAnswerText(message.blocks)
                : "";
            const liveActivityTail =
              message.role === "assistant" &&
              message.streaming === true &&
              rendered[rendered.length - 1]?.kind === "activityGroup";

            return (
              <div
                key={message.id}
                className={`assistant-chat-message ${message.role}${
                  failed ? " failed" : ""
                }${stopped ? " cancelled" : ""}`}
              >
                {message.role === "assistant" ? (
                  message.blocks.length === 0 && message.streaming ? (
                    <AssistantThinkingIndicator />
                  ) : (
                    <div className="assistant-chat-blocks">
                      {rendered.map((item, index) =>
                        item.kind === "activityGroup" ? (
                          <AssistantActivityGroup
                            key={item.blocks[0]!.id}
                            blocks={item.blocks}
                            active={
                              liveActivityTail && index === rendered.length - 1
                            }
                            liveBlockIds={liveBlockIds}
                            liveKind={liveKind}
                          />
                        ) : item.kind === "artifactGroup" ? (
                          <AssistantArtifactCard
                            key={item.blocks[0]!.id}
                            blocks={item.blocks}
                            chatId={chatId}
                            onAnswer={onAnswerArtifact}
                            onDefer={(id) =>
                              onDecideToolCall(id, false, false)
                            }
                          />
                        ) : item.kind === "askGroup" ? (
                          <AssistantAskUserCard
                            key={item.blocks[0]!.id}
                            blocks={item.blocks}
                            onAnswer={onAnswerAskUser}
                            onSkip={(id) =>
                              onDecideToolCall(id, false, false)
                            }
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
                              liveBlockIds.has(item.block.id) &&
                              liveKind === "reasoning"
                            }
                          />
                        ) : item.block.type === "text" ? (
                          <AssistantMarkdown
                            key={item.block.id}
                            content={item.block.content}
                            streaming={
                              liveBlockIds.has(item.block.id) &&
                              liveKind !== "reasoning"
                            }
                          />
                        ) : item.block.type === "steer" ? (
                          <AssistantSteerBlock
                            key={item.block.id}
                            block={item.block}
                          />
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
                          <AssistantToolCallBlock
                            key={item.block.id}
                            block={item.block}
                          />
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
                            Связь прервана. Повторная попытка{" "}
                            {retryState.attempt}/{retryState.maxAttempts} через{" "}
                            {retryState.delaySeconds} с.
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
                            Ответ обрезан: исчерпан лимит длины ответа.
                            Увеличьте «Лимит ответа» в настройках провайдера
                            или попросите продолжить.
                          </span>
                        </div>
                      ) : null}
                      {failed ? (
                        <div className="assistant-chat-error-card">
                          <AlertCircle size={13} aria-hidden />
                          <span>
                            {message.errorMessage ??
                              "Не удалось получить ответ"}
                          </span>
                          {message.contextLengthExceeded ? (
                            <button
                              type="button"
                              className="assistant-chat-error-retry"
                              onClick={() =>
                                onRetryWithCompaction(message.id)
                              }
                            >
                              Сжать историю и повторить
                            </button>
                          ) : null}
                        </div>
                      ) : null}
                      {!message.streaming &&
                      (typeof message.durationMs === "number" ||
                        answerText !== "") ? (
                        <div className="assistant-chat-answer-footer">
                          {typeof message.durationMs === "number" ? (
                            <span className="assistant-chat-duration">
                              Готово за{" "}
                              {formatElapsedDuration(
                                Math.round(message.durationMs / 1000),
                              )}
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
                  )
                ) : (
                  <AssistantUserMessage content={message.content} />
                )}
              </div>
            );
          })
        )}
      </div>
      {showJumpToBottom ? (
        <button
          type="button"
          className="assistant-scroll-to-bottom"
          onClick={onJumpToBottom}
        >
          <ArrowDown size={12} strokeWidth={2} aria-hidden />
          <span>Вниз</span>
        </button>
      ) : null}
    </div>
  );
}
