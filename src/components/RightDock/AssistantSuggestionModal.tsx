import { useState } from "react";
import { Lock } from "lucide-react";
import type { AiAccessMode } from "../../lib/aiTools";
import type { AssistantSuggestion } from "../../lib/assistantSuggestions";
import { needsAccessUpgrade, suggestionFormComplete } from "../../lib/assistantSuggestions";
import "../Welcome/CloneRepoModal.css";

type AssistantSuggestionModalProps = {
  suggestion: AssistantSuggestion;
  /** Seeded from values the user already gave earlier in the same branch —
   * see `prefillValues`. */
  initialValues: Record<string, string>;
  /** The assistant's *current* access mode, so the form can tell whether
   * submitting also means widening it to the whole repository. */
  accessMode: AiAccessMode;
  onCancel: () => void;
  onSubmit: (values: Record<string, string>) => void;
};

/** The «fill in the blanks» step between a suggestion chip and the composer.
 *
 * Two jobs, both of which the chip alone can't do: collect the `{{key}}`
 * values as real labelled fields (rather than leaving the prompt dangling for
 * the user to finish), and get explicit consent when the suggestion needs
 * full-repository access — the submit button itself is that consent, so the
 * escalation never happens on a stray chip click.
 *
 * Deliberately not a copy of `EditorContextActionModal`: that one is a single
 * anonymous field with no notion of access. The shared `clone-modal-*` chrome
 * is reused so both look native. */
export function AssistantSuggestionModal({
  suggestion,
  initialValues,
  accessMode,
  onCancel,
  onSubmit,
}: AssistantSuggestionModalProps) {
  const [values, setValues] = useState<Record<string, string>>(initialValues);
  const inputs = suggestion.inputs ?? [];
  const upgradesAccess = needsAccessUpgrade(suggestion, accessMode);
  const complete = suggestionFormComplete(suggestion, values);

  const handleSubmit = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!complete) return;
    onSubmit(values);
  };

  return (
    <div className="clone-modal-backdrop" role="presentation" onClick={onCancel}>
      <div
        className="clone-modal assistant-suggestion-modal"
        role="dialog"
        aria-labelledby="assistant-suggestion-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="assistant-suggestion-title">
          {suggestion.label}
        </div>
        {suggestion.hint ? (
          <p className="assistant-suggestion-modal-hint">{suggestion.hint}</p>
        ) : null}

        <form className="assistant-suggestion-modal-form" onSubmit={handleSubmit}>
          {inputs.map((input, index) => (
            <label className="assistant-suggestion-modal-field" key={input.key}>
              <span className="assistant-suggestion-modal-label">{input.label}</span>
              {input.multiline ? (
                <textarea
                  className="assistant-suggestion-modal-input"
                  rows={8}
                  placeholder={input.placeholder}
                  value={values[input.key] ?? ""}
                  autoFocus={index === 0}
                  onChange={(event) =>
                    setValues((prev) => ({ ...prev, [input.key]: event.target.value }))
                  }
                />
              ) : (
                <input
                  type="text"
                  className="assistant-suggestion-modal-input"
                  placeholder={input.placeholder}
                  value={values[input.key] ?? ""}
                  autoFocus={index === 0}
                  onChange={(event) =>
                    setValues((prev) => ({ ...prev, [input.key]: event.target.value }))
                  }
                />
              )}
            </label>
          ))}

          {upgradesAccess ? (
            <p className="assistant-suggestion-modal-access" role="note">
              <Lock size={13} strokeWidth={1.75} aria-hidden />
              <span>
                Подсказке нужен доступ ко всему репозиторию — сейчас ассистент видит только
                документацию.
              </span>
            </p>
          ) : null}

          <div className="clone-modal-actions">
            <button type="button" className="clone-modal-btn" onClick={onCancel}>
              Отмена
            </button>
            <button type="submit" className="clone-modal-btn primary" disabled={!complete}>
              {upgradesAccess ? "Включить доступ и вставить" : "Вставить в чат"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
