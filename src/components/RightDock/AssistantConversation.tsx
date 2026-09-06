import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useChatScrollFollow } from "../../hooks/useChatScrollFollow";
import { useChatSideEffects } from "../../hooks/useChatSideEffects";
import { useLlmChat } from "../../hooks/useLlmChat";
import { PLAN_EXECUTION_START_TEXT } from "../../lib/assistantConfig";
import {
  ASSISTANT_SUGGESTIONS,
  buildSuggestionContext,
  needsAccessUpgrade,
  needsSuggestionForm,
  renderSuggestionText,
  suggestionsForMode,
  visibleSuggestions,
  type AssistantSuggestion,
} from "../../lib/assistantSuggestions";
import type {
  AiAccessMode,
  ConversationMode,
  LlmToolDefinition,
  Task,
} from "../../lib/aiTools";
import {
  searchIsDegraded,
  type ChatMessage,
} from "../../lib/chatBlocks";
import { noteLlmChat } from "../../lib/llm";
import type {
  LlmProviderConfig,
  PendingApproval,
  ResolvedLlmProvider,
} from "../../lib/llm";
import type { SpecsRepoInfo } from "../../lib/openapi";
import type { UpdatedReference } from "../../lib/project";
import { openArtifactTab } from "../../lib/artifactTabs";
import { openVisualTab } from "../../lib/visuals";
import { PlanProgressWidget } from "./PlanProgressWidget";
import { TodoProgressWidget } from "./TodoProgressWidget";
import {
  AssistantComposer,
  formatSelectionForChat,
  type ChatAttachment,
} from "./AssistantConversation/AssistantComposer";
import {
  AssistantModelControls,
  CHAT_MODE_OPTIONS,
} from "./AssistantConversation/AssistantModelControls";
import { AssistantTranscript } from "./AssistantConversation/AssistantTranscript";

type AssistantConversationProps = {
  chatId: string | null;
  initialMessages: ChatMessage[];
  initialTodos: Task[];
  initialActivePlanId: string | null;
  initialPendingResume: PendingApproval | null;
  onTurnSettled: (
    messages: ChatMessage[],
    todos: Task[],
    activePlanId: string | null,
  ) => void;
  onTurnPaused: (
    messages: ChatMessage[],
    todos: Task[],
    activePlanId: string | null,
    pendingResume: PendingApproval,
  ) => void;
  onSendingChange: (sending: boolean) => void;
  providerId: string | null;
  accessMode: AiAccessMode;
  accessModeBusy: boolean;
  onAccessModeChange: (mode: AiAccessMode) => void;
  conversationMode: ConversationMode;
  onConversationModeChange: (mode: ConversationMode) => void;
  specsRepoInfo: SpecsRepoInfo | null;
  toolDefinitions: LlmToolDefinition[];
  docsRootRelativeToRepo: string | null;
  docsRoot: string;
  repoRoot: string;
  activeFilePath: string | null;
  hasUncommittedChanges: boolean;
  onFileWritten: (info: { tool: string; path: string }) => void;
  onFileMoved: (info: {
    from: string;
    to: string;
    updatedFiles: UpdatedReference[];
  }) => void;
  refreshAccessMode: () => Promise<void>;
  activeProvider: ResolvedLlmProvider | null;
  updateProviderConfig: (
    providerId: string,
    patch: Partial<Omit<LlmProviderConfig, "id">>,
  ) => Promise<void>;
  refreshLlmSetup: () => Promise<void>;
  followUpSuggestionsEnabled: boolean;
  taskDoneSoundEnabled: boolean;
  needAnswerSoundEnabled: boolean;
  chatInsertRequest: {
    id: number;
    text: string;
    filePath: string | null;
  } | null;
  onChatInsertHandled?: () => void;
  assistantSendRequest?: {
    id: number;
    text: string;
    conversationMode?: ConversationMode;
  } | null;
  onAssistantSendHandled?: () => void;
  assistantDraftRequest?: {
    id: number;
    text: string;
    conversationMode?: ConversationMode;
  } | null;
  onAssistantDraftHandled?: () => void;
};

export function AssistantConversation({
  chatId,
  initialMessages,
  initialTodos,
  initialActivePlanId,
  initialPendingResume,
  onTurnSettled,
  onTurnPaused,
  onSendingChange,
  providerId,
  accessMode,
  accessModeBusy,
  onAccessModeChange,
  conversationMode,
  onConversationModeChange,
  specsRepoInfo,
  toolDefinitions,
  docsRootRelativeToRepo,
  docsRoot,
  repoRoot,
  activeFilePath,
  hasUncommittedChanges,
  onFileWritten,
  onFileMoved,
  refreshAccessMode,
  activeProvider,
  updateProviderConfig,
  refreshLlmSetup,
  followUpSuggestionsEnabled,
  taskDoneSoundEnabled,
  needAnswerSoundEnabled,
  chatInsertRequest,
  onChatInsertHandled,
  assistantSendRequest,
  onAssistantSendHandled,
  assistantDraftRequest,
  onAssistantDraftHandled,
}: AssistantConversationProps) {
  const contextLimit = activeProvider?.limit?.context ?? null;
  const {
    messages,
    sending,
    retryState,
    pendingSteers,
    unsteerChat,
    sendMessage,
    steerChat,
    retryWithCompaction,
    stopChat,
    contextTokens,
    contextBreakdown,
    lastRequestTokens,
    decideToolCall,
    answerAskUser,
    answerArtifact,
    todos,
    clearTodos,
    activePlanId,
    setActivePlanId,
  } = useLlmChat(
    providerId,
    contextLimit,
    accessMode,
    conversationMode,
    specsRepoInfo,
    toolDefinitions,
    docsRootRelativeToRepo,
    initialMessages,
    initialTodos,
    initialActivePlanId,
    initialPendingResume,
    onTurnSettled,
    onTurnPaused,
    activeFilePath,
    taskDoneSoundEnabled,
    needAnswerSoundEnabled,
  );

  const pendingStartPlanRef = useRef(false);
  const pendingAssistantSendRef = useRef<{
    text: string;
    conversationMode: ConversationMode;
  } | null>(null);

  const runAssistantSend = useCallback(
    (text: string, targetMode: ConversationMode = "agent") => {
      if (sending || !text.trim()) return;
      if (conversationMode === targetMode) {
        void sendMessage(text);
        return;
      }
      pendingAssistantSendRef.current = {
        text,
        conversationMode: targetMode,
      };
      onConversationModeChange(targetMode);
    },
    [
      conversationMode,
      sending,
      sendMessage,
      onConversationModeChange,
    ],
  );

  useEffect(() => {
    const pending = pendingAssistantSendRef.current;
    if (
      !pending ||
      conversationMode !== pending.conversationMode ||
      sending
    ) {
      return;
    }
    pendingAssistantSendRef.current = null;
    void sendMessage(pending.text);
  }, [conversationMode, sending, sendMessage]);

  const startPlan = useCallback(
    (planId: string) => {
      setActivePlanId(planId);
      if (conversationMode === "agent") {
        if (!sending) {
          void sendMessage(PLAN_EXECUTION_START_TEXT, {
            planExecutionStart: true,
          });
        }
        return;
      }
      pendingStartPlanRef.current = true;
      onConversationModeChange("agent");
    },
    [
      conversationMode,
      sending,
      sendMessage,
      setActivePlanId,
      onConversationModeChange,
    ],
  );

  useEffect(() => {
    if (
      conversationMode !== "agent" ||
      !pendingStartPlanRef.current ||
      sending
    ) {
      return;
    }
    pendingStartPlanRef.current = false;
    void sendMessage(PLAN_EXECUTION_START_TEXT, {
      planExecutionStart: true,
    });
  }, [conversationMode, sending, sendMessage]);

  useEffect(() => {
    const onStart = (event: Event) => {
      const planId = (event as CustomEvent<{ planId?: string }>).detail
        ?.planId;
      if (!planId || sending) return;
      startPlan(planId);
    };
    window.addEventListener("atlas-start-plan", onStart);
    return () => window.removeEventListener("atlas-start-plan", onStart);
  }, [sending, startPlan]);

  useChatSideEffects({
    messages,
    initialMessages,
    sending,
    onSendingChange,
    refreshAccessMode,
    onConversationModeChange,
    onFileWritten,
    onFileMoved,
  });

  const [draft, setDraft] = useState("");
  const [attachments, setAttachments] = useState<ChatAttachment[]>([]);
  const [attachmentsExpanded, setAttachmentsExpanded] = useState(false);
  const [activeSuggestion, setActiveSuggestion] =
    useState<AssistantSuggestion | null>(null);
  const [formSuggestion, setFormSuggestion] =
    useState<AssistantSuggestion | null>(null);
  const [rememberedValues, setRememberedValues] = useState<
    Record<string, string>
  >({});
  const [hoveredSuggestion, setHoveredSuggestion] =
    useState<AssistantSuggestion | null>(null);
  const chatInputRef = useRef<HTMLTextAreaElement>(null);
  const lastHandledChatInsertIdRef = useRef(0);

  const suggestionCtx = useMemo(
    () =>
      buildSuggestionContext({
        conversationMode,
        activeFilePath,
        hasUncommittedChanges,
      }),
    [conversationMode, activeFilePath, hasUncommittedChanges],
  );
  const suggestionsByMode = useMemo(
    () =>
      CHAT_MODE_OPTIONS.map((option) => ({
        mode: option.value,
        items: suggestionsForMode(ASSISTANT_SUGGESTIONS, {
          ...suggestionCtx,
          conversationMode: option.value,
        }),
      })),
    [suggestionCtx],
  );
  const hasAnySuggestion = suggestionsByMode.some(
    (group) => group.items.length > 0,
  );

  useEffect(() => {
    setHoveredSuggestion(null);
  }, [conversationMode]);

  const followUpSuggestions = useMemo(
    () =>
      visibleSuggestions(
        activeSuggestion?.followUps ?? [],
        suggestionCtx,
      ),
    [activeSuggestion, suggestionCtx],
  );
  const showFollowUpBar =
    followUpSuggestionsEnabled &&
    messages.length > 0 &&
    followUpSuggestions.length > 0;
  const embeddingsUnavailable = searchIsDegraded(messages);

  const {
    scrollRef,
    showJumpToBottom,
    onScroll,
    onWheel,
    jumpToBottom,
    followNextResponse,
  } = useChatScrollFollow(messages, messages.length);

  useEffect(() => {
    if (
      !chatInsertRequest ||
      chatInsertRequest.id === lastHandledChatInsertIdRef.current
    ) {
      return;
    }
    lastHandledChatInsertIdRef.current = chatInsertRequest.id;
    if (chatInsertRequest.text.trim()) {
      setAttachments((current) => [
        ...current,
        {
          id: chatInsertRequest.id,
          text: chatInsertRequest.text,
          filePath: chatInsertRequest.filePath,
        },
      ]);
      chatInputRef.current?.focus();
    }
    onChatInsertHandled?.();
  }, [chatInsertRequest, onChatInsertHandled]);

  const lastHandledAssistantSendIdRef = useRef<number | null>(null);
  useEffect(() => {
    if (
      !assistantSendRequest ||
      assistantSendRequest.id === lastHandledAssistantSendIdRef.current
    ) {
      return;
    }
    lastHandledAssistantSendIdRef.current = assistantSendRequest.id;
    runAssistantSend(
      assistantSendRequest.text,
      assistantSendRequest.conversationMode ?? "agent",
    );
    onAssistantSendHandled?.();
  }, [
    assistantSendRequest,
    runAssistantSend,
    onAssistantSendHandled,
  ]);

  const lastHandledAssistantDraftIdRef = useRef<number | null>(null);
  useEffect(() => {
    if (
      !assistantDraftRequest ||
      assistantDraftRequest.id === lastHandledAssistantDraftIdRef.current
    ) {
      return;
    }
    lastHandledAssistantDraftIdRef.current = assistantDraftRequest.id;
    const targetMode = assistantDraftRequest.conversationMode;
    if (targetMode && targetMode !== conversationMode) {
      onConversationModeChange(targetMode);
    }
    setDraft(assistantDraftRequest.text);
    requestAnimationFrame(() => {
      const input = chatInputRef.current;
      if (!input) return;
      input.focus();
      input.setSelectionRange(input.value.length, input.value.length);
    });
    onAssistantDraftHandled?.();
  }, [
    assistantDraftRequest,
    conversationMode,
    onConversationModeChange,
    onAssistantDraftHandled,
  ]);

  const applySuggestion = (
    suggestion: AssistantSuggestion,
    values: Record<string, string>,
  ) => {
    if (
      suggestion.mode &&
      suggestion.mode !== conversationMode
    ) {
      onConversationModeChange(suggestion.mode);
    }
    if (needsAccessUpgrade(suggestion, accessMode)) {
      onAccessModeChange("fullRepo");
    }
    setRememberedValues((current) => ({
      ...current,
      ...values,
    }));
    setDraft(renderSuggestionText(suggestion, values));
    setActiveSuggestion(suggestion);
    requestAnimationFrame(() => {
      const input = chatInputRef.current;
      if (!input) return;
      input.focus();
      input.setSelectionRange(input.value.length, input.value.length);
    });
  };

  const handleSuggestionClick = (
    suggestion: AssistantSuggestion,
  ) => {
    if (needsSuggestionForm(suggestion, accessMode)) {
      setFormSuggestion(suggestion);
      return;
    }
    applySuggestion(suggestion, {});
  };

  const handleSuggestionFormSubmit = (
    values: Record<string, string>,
  ) => {
    if (!formSuggestion) return;
    applySuggestion(formSuggestion, values);
    setFormSuggestion(null);
  };

  const handleSend = () => {
    const text = draft.trim();
    if (sending) {
      if (!text) return;
      setDraft("");
      void steerChat(text).catch((error) => {
        console.error("Не удалось добавить уточнение", error);
        setDraft((current) => current || text);
      });
      return;
    }
    if (!text && attachments.length === 0) return;
    const quotes = attachments.map((attachment) =>
      formatSelectionForChat(
        attachment.text,
        attachment.filePath,
      ),
    );
    const combined = [...quotes, text]
      .filter(Boolean)
      .join("\n\n");
    setDraft("");
    setAttachments([]);
    setAttachmentsExpanded(false);
    followNextResponse();
    void sendMessage(combined);
  };

  return (
    <>
      <TodoProgressWidget
        tasks={todos}
        onClearAll={sending ? undefined : clearTodos}
      />
      {activePlanId ? (
        <PlanProgressWidget
          planId={activePlanId}
          refreshKey={messages.length}
        />
      ) : null}
      <AssistantTranscript
        chatId={chatId}
        messages={messages}
        sending={sending}
        retryState={retryState}
        conversationMode={conversationMode}
        suggestionsByMode={suggestionsByMode}
        hasAnySuggestion={hasAnySuggestion}
        hoveredSuggestion={hoveredSuggestion}
        docsRoot={docsRoot}
        repoRoot={repoRoot}
        scrollRef={scrollRef}
        showJumpToBottom={showJumpToBottom}
        onScroll={onScroll}
        onWheel={onWheel}
        onJumpToBottom={jumpToBottom}
        onConversationModeChange={onConversationModeChange}
        onSuggestionClick={handleSuggestionClick}
        onSuggestionHoverChange={setHoveredSuggestion}
        onAnswerArtifact={answerArtifact}
        onAnswerAskUser={answerAskUser}
        onDecideToolCall={decideToolCall}
        onRetryWithCompaction={retryWithCompaction}
        onStartPlan={startPlan}
        onOpenPlan={(planId) => {
          window.dispatchEvent(
            new CustomEvent("atlas-open-plan", {
              detail: { planId },
            }),
          );
        }}
        onOpenArtifact={openArtifactTab}
        onOpenVisual={openVisualTab}
        onVisualRenderError={(note) => void noteLlmChat(note)}
        onRedrawVisual={(request) => {
          if (sending) void steerChat(request);
          else void sendMessage(request);
        }}
      />
      <AssistantModelControls
        providerId={providerId}
        activeProvider={activeProvider}
        sending={sending}
        accessMode={accessMode}
        accessModeBusy={accessModeBusy}
        conversationMode={conversationMode}
        contextTokens={contextTokens}
        contextBreakdown={contextBreakdown}
        lastRequestTokens={lastRequestTokens}
        onConversationModeChange={onConversationModeChange}
        onAccessModeChange={onAccessModeChange}
        updateProviderConfig={updateProviderConfig}
        refreshLlmSetup={refreshLlmSetup}
      />
      <AssistantComposer
        draft={draft}
        sending={sending}
        embeddingsUnavailable={embeddingsUnavailable}
        showFollowUpBar={showFollowUpBar}
        followUpSuggestions={followUpSuggestions}
        formSuggestion={formSuggestion}
        rememberedValues={rememberedValues}
        activeFilePath={activeFilePath}
        conversationMode={conversationMode}
        accessMode={accessMode}
        pendingSteers={pendingSteers}
        attachments={attachments}
        attachmentsExpanded={attachmentsExpanded}
        inputRef={chatInputRef}
        onDraftChange={setDraft}
        onSend={handleSend}
        onStop={stopChat}
        onUnsteer={(id) => void unsteerChat(id)}
        onRemoveAttachment={(id) =>
          setAttachments((current) =>
            current.filter((attachment) => attachment.id !== id),
          )
        }
        onAttachmentsExpandedChange={setAttachmentsExpanded}
        onSuggestionClick={handleSuggestionClick}
        onDismissFollowUps={() => setActiveSuggestion(null)}
        onCancelSuggestionForm={() => setFormSuggestion(null)}
        onSubmitSuggestionForm={handleSuggestionFormSubmit}
      />
    </>
  );
}
