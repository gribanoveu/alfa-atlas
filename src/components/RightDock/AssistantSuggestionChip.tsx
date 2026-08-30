import { Eye, Lock, Pencil } from "lucide-react";
import type { AssistantSuggestion } from "../../lib/assistantSuggestions";
import { suggestionAccess } from "../../lib/assistantSuggestions";

/** One suggestion chip. The badges are the whole point of the component: a
 * chip has to say, before it is clicked, whether the assistant will touch
 * files (pencil) or only read (eye), and whether it needs the whole
 * repository rather than just the docs subtree (lock). */
export function AssistantSuggestionChip({
  suggestion,
  className,
  disabled,
  onClick,
  onHoverChange,
}: {
  suggestion: AssistantSuggestion;
  className: string;
  disabled?: boolean;
  onClick: () => void;
  onHoverChange?: (suggestion: AssistantSuggestion | null) => void;
}) {
  const title = [suggestion.hint, suggestion.writes ? "правит файлы" : "только чтение"]
    .filter(Boolean)
    .join(" · ");
  return (
    <button
      type="button"
      className={className}
      title={title}
      disabled={disabled}
      onClick={onClick}
      onMouseEnter={() => onHoverChange?.(suggestion)}
      onMouseLeave={() => onHoverChange?.(null)}
      onFocus={() => onHoverChange?.(suggestion)}
      onBlur={() => onHoverChange?.(null)}
    >
      <span className="assistant-suggestion-chip-label">{suggestion.label}</span>
      <span className="assistant-suggestion-chip-badges" aria-hidden>
        {suggestionAccess(suggestion) === "fullRepo" ? <Lock size={11} strokeWidth={1.75} /> : null}
        {suggestion.writes ? (
          <Pencil size={11} strokeWidth={1.75} />
        ) : (
          <Eye size={11} strokeWidth={1.75} />
        )}
      </span>
    </button>
  );
}
