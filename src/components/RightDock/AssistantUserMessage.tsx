import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";

/** The text of a sent user message plus a hover-revealed copy button — the
 * message is the exact prompt that was sent, so copying it is how someone
 * reuses it as the opening message of a new chat. */
export function AssistantUserMessage({ content }: { content: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);

  const handleCopy = async () => {
    try {
      await writeText(content);
      setCopied(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard unavailable — the text stays visible and selectable.
    }
  };

  return (
    <>
      {content}
      <button
        type="button"
        className={`assistant-chat-user-copy${copied ? " copied" : ""}`}
        title={copied ? "Скопировано" : "Копировать сообщение"}
        aria-label={copied ? "Скопировано" : "Копировать сообщение"}
        onClick={() => void handleCopy()}
      >
        {copied ? <Check size={12} aria-hidden /> : <Copy size={12} aria-hidden />}
      </button>
    </>
  );
}
