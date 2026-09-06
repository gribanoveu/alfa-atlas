import { CopyTextButton } from "./CopyTextButton";

/** The text of a sent user message plus a hover-revealed copy button — the
 * message is the exact prompt that was sent, so copying it is how someone
 * reuses it as the opening message of a new chat. */
export function AssistantUserMessage({ content }: { content: string }) {
  return (
    <>
      {content}
      <CopyTextButton
        text={content}
        className="assistant-chat-user-copy"
        label="Копировать сообщение"
      />
    </>
  );
}
