import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";

/** Hover-revealed copy button with a short "скопировано" confirmation —
 * shared by the user bubble and the assistant answer footer so the two
 * can't drift in timing or in what a failed clipboard write does. */
export function CopyTextButton({
  text,
  className,
  label,
  size = 12,
}: {
  text: string;
  className: string;
  label: string;
  size?: number;
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);

  const handleCopy = async () => {
    try {
      await writeText(text);
      setCopied(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard unavailable — the text stays visible and selectable.
    }
  };

  return (
    <button
      type="button"
      className={`${className}${copied ? " copied" : ""}`}
      title={copied ? "Скопировано" : label}
      aria-label={copied ? "Скопировано" : label}
      onClick={() => void handleCopy()}
    >
      {copied ? <Check size={size} aria-hidden /> : <Copy size={size} aria-hidden />}
    </button>
  );
}
