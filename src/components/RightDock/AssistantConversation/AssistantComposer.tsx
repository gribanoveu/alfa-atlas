import type { RefObject } from "react";
import {
  AlertCircle,
  ChevronUp,
  Clock3,
  FileText,
  Send,
  Sparkles,
  Square,
  X,
} from "lucide-react";
import { CHAT_INPUT_ROWS } from "../../../lib/assistantConfig";
import {
  prefillValues,
  suggestionBlockedReason,
  type AssistantSuggestion,
} from "../../../lib/assistantSuggestions";
import type { AiAccessMode, ConversationMode } from "../../../lib/aiTools";
import { AssistantSuggestionChip } from "../AssistantSuggestionChip";
import { AssistantSuggestionModal } from "../AssistantSuggestionModal";

export type ChatAttachment = {
  id: number;
  text: string;
  filePath: string | null;
};

export function formatSelectionForChat(
  text: string,
  filePath: string | null,
): string {
  const pathLine = filePath ? `Из \`${filePath}\`:\n` : "";
  const quoted = text
    .split("\n")
    .map((line) => (line.trim() ? `> ${line}` : ">"))
    .join("\n");
  return `${pathLine}${quoted}`;
}

function attachmentLabel(attachment: ChatAttachment): string {
  const lines = attachment.text.split("\n").length;
  const sizeLabel =
    lines > 1 ? `${lines} строк` : `${attachment.text.length} симв.`;
  const name = attachment.filePath?.split(/[/\\]/).pop();
  return name
    ? `${name} · ${sizeLabel}`
    : `Фрагмент · ${sizeLabel}`;
}

const ATTACHMENTS_INLINE_LIMIT = 2;

function AttachmentChip({
  attachment,
  variant,
  onRemove,
}: {
  attachment: ChatAttachment;
  variant: "chip" | "row";
  onRemove: () => void;
}) {
  const label = attachmentLabel(attachment);
  return (
    <span
      className={
        variant === "chip"
          ? "assistant-chat-attachment-chip"
          : "assistant-chat-attachment-row"
      }
      role="listitem"
      title={attachment.text}
    >
      <FileText size={12} strokeWidth={1.75} aria-hidden />
      <span className="assistant-chat-attachment-label">{label}</span>
      <button
        type="button"
        className="assistant-chat-attachment-remove"
        aria-label={`Убрать вложение: ${label}`}
        onClick={onRemove}
      >
        <X size={10} strokeWidth={2} aria-hidden />
      </button>
    </span>
  );
}

function FollowUpChip({
  suggestion,
  sending,
  conversationMode,
  accessMode,
  onClick,
}: {
  suggestion: AssistantSuggestion;
  sending: boolean;
  conversationMode: ConversationMode;
  accessMode: AiAccessMode;
  onClick: () => void;
}) {
  const blocked = suggestionBlockedReason(suggestion, {
    sending,
    conversationMode,
    accessMode,
  });
  return (
    <AssistantSuggestionChip
      suggestion={suggestion}
      className="assistant-followup-chip"
      disabled={blocked !== null}
      {...(blocked ? { disabledReason: blocked } : {})}
      onClick={onClick}
    />
  );
}

type AssistantComposerProps = {
  draft: string;
  sending: boolean;
  embeddingsUnavailable: boolean;
  showFollowUpBar: boolean;
  followUpSuggestions: AssistantSuggestion[];
  formSuggestion: AssistantSuggestion | null;
  rememberedValues: Record<string, string>;
  activeFilePath: string | null;
  conversationMode: ConversationMode;
  accessMode: AiAccessMode;
  pendingSteers: { id: string; text: string }[];
  attachments: ChatAttachment[];
  attachmentsExpanded: boolean;
  inputRef: RefObject<HTMLTextAreaElement | null>;
  onDraftChange: (draft: string) => void;
  onSend: () => void;
  onStop: () => void;
  onUnsteer: (id: string) => void;
  onRemoveAttachment: (id: number) => void;
  onAttachmentsExpandedChange: (expanded: boolean) => void;
  onSuggestionClick: (suggestion: AssistantSuggestion) => void;
  onDismissFollowUps: () => void;
  onCancelSuggestionForm: () => void;
  onSubmitSuggestionForm: (values: Record<string, string>) => void;
};

export function AssistantComposer({
  draft,
  sending,
  embeddingsUnavailable,
  showFollowUpBar,
  followUpSuggestions,
  formSuggestion,
  rememberedValues,
  activeFilePath,
  conversationMode,
  accessMode,
  pendingSteers,
  attachments,
  attachmentsExpanded,
  inputRef,
  onDraftChange,
  onSend,
  onStop,
  onUnsteer,
  onRemoveAttachment,
  onAttachmentsExpandedChange,
  onSuggestionClick,
  onDismissFollowUps,
  onCancelSuggestionForm,
  onSubmitSuggestionForm,
}: AssistantComposerProps) {
  return (
    <>
      {embeddingsUnavailable ? (
        <div className="assistant-degraded-bar" role="status">
          <AlertCircle size={13} strokeWidth={1.75} aria-hidden />
          <span>
            API эмбеддингов недоступен — поиск идёт только по именам и тексту,
            результаты могут быть хуже.
          </span>
        </div>
      ) : null}
      {showFollowUpBar ? (
        <div
          className="assistant-followup-bar"
          role="group"
          aria-label="Похожие предложения"
        >
          <Sparkles
            className="assistant-followup-bar-icon"
            size={12}
            strokeWidth={1.75}
            aria-hidden
          />
          <div className="assistant-followup-bar-chips">
            {followUpSuggestions.map((suggestion) => (
              <FollowUpChip
                key={suggestion.id}
                suggestion={suggestion}
                sending={sending}
                conversationMode={conversationMode}
                accessMode={accessMode}
                onClick={() => onSuggestionClick(suggestion)}
              />
            ))}
          </div>
          <button
            type="button"
            className="assistant-followup-bar-dismiss"
            aria-label="Скрыть предложения"
            onClick={onDismissFollowUps}
          >
            <X size={12} aria-hidden />
          </button>
        </div>
      ) : null}
      <div
        className={`assistant-chat-input-row${
          showFollowUpBar ? " has-followups" : ""
        }`}
      >
        <div className="assistant-chat-input-wrap">
          {pendingSteers.length > 0 ? (
            <div
              className="assistant-steer-pending-list"
              role="status"
              aria-label="Уточнения в очереди"
            >
              {pendingSteers.map((steer) => (
                <div className="assistant-steer-pending" key={steer.id}>
                  <Clock3 size={12} strokeWidth={1.75} aria-hidden />
                  <span className="assistant-steer-pending-text">
                    {steer.text}
                  </span>
                  <span className="assistant-steer-pending-label">
                    В очереди
                  </span>
                  <button
                    type="button"
                    className="assistant-steer-pending-cancel"
                    title="Отменить уточнение"
                    aria-label={`Отменить уточнение: ${steer.text}`}
                    onClick={() => onUnsteer(steer.id)}
                  >
                    <X size={12} aria-hidden />
                  </button>
                </div>
              ))}
            </div>
          ) : null}
          {attachments.length === 0 ? null : attachments.length <=
            ATTACHMENTS_INLINE_LIMIT ? (
            <div className="assistant-chat-attachments" role="list">
              {attachments.map((attachment) => (
                <AttachmentChip
                  key={attachment.id}
                  attachment={attachment}
                  variant="chip"
                  onRemove={() => onRemoveAttachment(attachment.id)}
                />
              ))}
            </div>
          ) : !attachmentsExpanded ? (
            <div className="assistant-chat-attachments" role="list">
              {attachments
                .slice(0, ATTACHMENTS_INLINE_LIMIT)
                .map((attachment) => (
                  <AttachmentChip
                    key={attachment.id}
                    attachment={attachment}
                    variant="chip"
                    onRemove={() => onRemoveAttachment(attachment.id)}
                  />
                ))}
              <button
                type="button"
                className="assistant-chat-attachments-toggle"
                onClick={() => onAttachmentsExpandedChange(true)}
              >
                Все ({attachments.length})
              </button>
            </div>
          ) : (
            <div className="assistant-chat-attachments-list">
              <button
                type="button"
                className="assistant-chat-attachments-toggle"
                onClick={() => onAttachmentsExpandedChange(false)}
              >
                <ChevronUp size={12} aria-hidden />
                Свернуть
              </button>
              <div
                className="assistant-chat-attachments-list-items"
                role="list"
              >
                {attachments.map((attachment) => (
                  <AttachmentChip
                    key={attachment.id}
                    attachment={attachment}
                    variant="row"
                    onRemove={() => onRemoveAttachment(attachment.id)}
                  />
                ))}
              </div>
            </div>
          )}
          <textarea
            ref={inputRef}
            className="assistant-chat-input"
            rows={CHAT_INPUT_ROWS}
            value={draft}
            placeholder={
              sending
                ? "Уточнение…\n(Enter — добавить в работу, Shift+Enter — новая строка)"
                : "Спросите что-нибудь…\n(Enter — отправить, Shift+Enter — новая строка)"
            }
            onChange={(event) => onDraftChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                onSend();
              }
            }}
          />
          <div className="assistant-chat-input-tools">
            {sending ? (
              <button
                type="button"
                className="assistant-chat-stop"
                aria-label="Остановить"
                title="Остановить ответ ассистента"
                onClick={onStop}
              >
                <Square
                  size={13}
                  strokeWidth={1.75}
                  fill="currentColor"
                  aria-hidden
                />
              </button>
            ) : (
              <button
                type="button"
                className="assistant-chat-send"
                disabled={!draft.trim() && attachments.length === 0}
                aria-label="Отправить"
                onClick={onSend}
              >
                <Send size={15} strokeWidth={1.75} aria-hidden />
              </button>
            )}
          </div>
        </div>
      </div>
      {formSuggestion ? (
        <AssistantSuggestionModal
          suggestion={formSuggestion}
          initialValues={prefillValues(
            formSuggestion,
            rememberedValues,
            activeFilePath,
          )}
          accessMode={accessMode}
          onCancel={onCancelSuggestionForm}
          onSubmit={onSubmitSuggestionForm}
        />
      ) : null}
    </>
  );
}
