import { Loader2 } from "lucide-react";

import type { ChatMessage } from "../../lib/chatBlocks";

/** The two faces of one history-compaction pass (see
 * `src/lib/contextCompaction.ts`), keyed off `compactionRunning`.
 *
 * While the summarizer round trip is in flight this is a card — deliberately
 * louder than the settled form, because it's explaining a pause the user is
 * sitting through rather than annotating something already done. Once the
 * pass settles it collapses to the centered pill it has always been: a
 * system event, styled unlike a chat bubble so it reads as neither party
 * having "said" it.
 *
 * A message restored from `chat_store` can only ever be the settled form —
 * `compactionRunning` never survives to disk (see its doc comment on
 * `ChatMessage`) — so the spinner cannot come back to life on reload. */
export function AssistantCompactionNotice({ message }: { message: ChatMessage }) {
  const text = message.role === "assistant" && message.blocks[0]?.type === "text" ? message.blocks[0].content : "";

  if (message.role === "assistant" && message.compactionRunning) {
    return (
      <div className="assistant-compaction-card is-running" role="status">
        <div className="assistant-plan-card-header">
          <span className="assistant-plan-card-eyebrow">Контекст</span>
          <div className="assistant-plan-card-title assistant-plan-card-title-live">
            <Loader2 className="assistant-chat-tool-spinner" size={15} aria-hidden />
            {text}
          </div>
        </div>
        <p className="assistant-plan-card-overview">
          Старые сообщения сворачиваются в резюме, чтобы диалог поместился в контекст модели.
        </p>
      </div>
    );
  }

  return (
    <div className="assistant-chat-compaction-notice">
      <span>{text}</span>
    </div>
  );
}
