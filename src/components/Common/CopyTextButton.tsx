import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";

/** The app's copy-to-clipboard button: an icon that flips to a tick for
 * 1.5 s, and swallows a failed clipboard write because the text stays on
 * screen and selectable either way.
 *
 * Every surface uses this one rather than its own copy — fifteen call sites
 * had reimplemented it, each with an uncleared `setTimeout` and, in half of
 * them, a `copiedId`/`onCopy` pair threaded down through props to say which
 * row was ticked. The state belongs to the button.
 *
 * `label` is the tooltip and, unless `ariaLabel` overrides it, the idle
 * accessible name — several call sites want a generic "Копировать" tooltip
 * next to a specific name for screen readers ("Скопировать код 404"). Once
 * copied, both become "Скопировано": the confirmation is the point. */
export function CopyTextButton({
  text,
  className,
  label,
  ariaLabel,
  size = 12,
  disabled = false,
}: {
  text: string;
  className: string;
  label: string;
  ariaLabel?: string;
  size?: number;
  disabled?: boolean;
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
      // The tick is also announced, not just drawn — `ariaLabel` only renames
      // the idle state.
      aria-label={copied ? "Скопировано" : (ariaLabel ?? label)}
      disabled={disabled}
      onClick={() => void handleCopy()}
    >
      {copied ? (
        <Check size={size} strokeWidth={2} aria-hidden />
      ) : (
        <Copy size={size} strokeWidth={1.75} aria-hidden />
      )}
    </button>
  );
}
