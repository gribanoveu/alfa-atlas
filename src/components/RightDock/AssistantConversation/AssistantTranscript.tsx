import type {
  RefObject,
  UIEventHandler,
  WheelEventHandler,
} from "react";
import { ArrowDown, Sparkles } from "lucide-react";
import type { AssistantSuggestion } from "../../../lib/assistantSuggestions";
import type { ConversationMode } from "../../../lib/aiTools";
import type { ChatMessage } from "../../../lib/chatBlocks";
import type { AskUserAnswerPayload } from "../../../lib/llm";
import type { Visual } from "../../../lib/visuals";
import { AssistantCompactionNotice } from "../AssistantCompactionNotice";
import { AssistantSuggestionChip } from "../AssistantSuggestionChip";
import { AssistantMessage, type RetryState } from "./AssistantMessage";
import { CHAT_MODE_OPTIONS } from "./AssistantModelControls";

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
          messages.map((message) =>
            message.role === "assistant" && message.isCompactionNotice ? (
              <AssistantCompactionNotice key={message.id} message={message} />
            ) : (
              <AssistantMessage
                key={message.id}
                message={message}
                chatId={chatId}
                docsRoot={docsRoot}
                repoRoot={repoRoot}
                sending={sending}
                // Only the streaming bubble can show a retry notice, and
                // handing `null` to the rest keeps this prop stable for them
                // — otherwise a retry would re-render the settled transcript.
                retryState={
                  message.role === "assistant" && message.streaming
                    ? retryState
                    : null
                }
                onAnswerArtifact={onAnswerArtifact}
                onAnswerAskUser={onAnswerAskUser}
                onDecideToolCall={onDecideToolCall}
                onRetryWithCompaction={onRetryWithCompaction}
                onStartPlan={onStartPlan}
                onOpenPlan={onOpenPlan}
                onOpenArtifact={onOpenArtifact}
                onOpenVisual={onOpenVisual}
                onVisualRenderError={onVisualRenderError}
                onRedrawVisual={onRedrawVisual}
              />
            ),
          )
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
