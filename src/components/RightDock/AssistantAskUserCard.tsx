import { useMemo, useState } from "react";
import type { AskUserAnswerPayload } from "../../lib/llm";
import type { ToolCallBlock } from "../../lib/chatBlocks";

type AskOption = { id: string; label: string };
type AskQuestion = {
  id: string;
  prompt: string;
  options: AskOption[];
  allowMultiple: boolean;
  allowCustom: boolean;
};
type AskArgs = {
  title: string | null;
  questions: AskQuestion[];
};

type AssistantAskUserCardProps = {
  blocks: ToolCallBlock[];
  onAnswer: (id: string, answer: AskUserAnswerPayload) => void;
  onSkip: (id: string) => void;
};

function parseAskArgs(argumentsJson: string): AskArgs | null {
  try {
    const parsed = JSON.parse(argumentsJson) as Record<string, unknown>;
    if (!parsed || typeof parsed !== "object") return null;
    const rawQuestions = parsed.questions;
    if (!Array.isArray(rawQuestions) || rawQuestions.length === 0) return null;
    const questions: AskQuestion[] = [];
    for (const raw of rawQuestions) {
      if (!raw || typeof raw !== "object") continue;
      const q = raw as Record<string, unknown>;
      if (typeof q.id !== "string" || typeof q.prompt !== "string" || !Array.isArray(q.options)) {
        continue;
      }
      const options: AskOption[] = [];
      for (const o of q.options) {
        if (!o || typeof o !== "object") continue;
        const opt = o as Record<string, unknown>;
        if (typeof opt.id === "string" && typeof opt.label === "string") {
          options.push({ id: opt.id, label: opt.label });
        }
      }
      if (options.length < 2) continue;
      questions.push({
        id: q.id,
        prompt: q.prompt,
        options,
        allowMultiple: Boolean(q.allowMultiple),
        allowCustom: Boolean(q.allowCustom),
      });
    }
    if (questions.length === 0) return null;
    return {
      title: typeof parsed.title === "string" ? parsed.title : null,
      questions,
    };
  } catch {
    return null;
  }
}

/** Mid-turn clarifying-question card for one or more pending `askUser`
 * calls (usually one). No countdown — the turn waits until the user
 * submits or skips (or Stop). */
export function AssistantAskUserCard({ blocks, onAnswer, onSkip }: AssistantAskUserCardProps) {
  const [decided, setDecided] = useState(false);
  // One form state per block id — rounds with multiple askUser calls are rare
  // but share one card shell when they share `askGroupId`.
  const parsedById = useMemo(() => {
    const map = new Map<string, AskArgs | null>();
    for (const b of blocks) map.set(b.id, parseAskArgs(b.argumentsJson));
    return map;
  }, [blocks]);

  const [selected, setSelected] = useState<Record<string, Record<string, string[]>>>(() => {
    const init: Record<string, Record<string, string[]>> = {};
    for (const b of blocks) {
      const args = parseAskArgs(b.argumentsJson);
      init[b.id] = {};
      if (args) {
        for (const q of args.questions) init[b.id]![q.id] = [];
      }
    }
    return init;
  });
  const [customText, setCustomText] = useState<Record<string, Record<string, string>>>(() => {
    const init: Record<string, Record<string, string>> = {};
    for (const b of blocks) {
      const args = parseAskArgs(b.argumentsJson);
      init[b.id] = {};
      if (args) {
        for (const q of args.questions) init[b.id]![q.id] = "";
      }
    }
    return init;
  });

  const canSubmitBlock = (blockId: string, args: AskArgs): boolean => {
    for (const q of args.questions) {
      const picks = selected[blockId]?.[q.id] ?? [];
      const custom = (customText[blockId]?.[q.id] ?? "").trim();
      if (picks.length === 0 && !(q.allowCustom && custom)) return false;
    }
    return true;
  };

  const allReady = blocks.every((b) => {
    const args = parsedById.get(b.id);
    return args ? canSubmitBlock(b.id, args) : false;
  });

  const toggleOption = (blockId: string, q: AskQuestion, optionId: string) => {
    if (decided) return;
    setSelected((prev) => {
      const blockSel = { ...(prev[blockId] ?? {}) };
      const current = blockSel[q.id] ?? [];
      if (q.allowMultiple) {
        blockSel[q.id] = current.includes(optionId)
          ? current.filter((id) => id !== optionId)
          : [...current, optionId];
      } else {
        blockSel[q.id] = current.includes(optionId) ? [] : [optionId];
      }
      return { ...prev, [blockId]: blockSel };
    });
  };

  const handleSubmit = () => {
    if (decided || !allReady) return;
    setDecided(true);
    for (const block of blocks) {
      const args = parsedById.get(block.id);
      if (!args) {
        onSkip(block.id);
        continue;
      }
      const answers: AskUserAnswerPayload["answers"] = args.questions.map((q) => {
        const ids = selected[block.id]?.[q.id] ?? [];
        const labels = q.options.filter((o) => ids.includes(o.id)).map((o) => o.label);
        const custom = (customText[block.id]?.[q.id] ?? "").trim();
        return {
          questionId: q.id,
          selectedOptionIds: ids,
          selectedLabels: labels,
          customText: q.allowCustom && custom ? custom : null,
        };
      });
      onAnswer(block.id, { answers });
    }
  };

  const handleSkip = () => {
    if (decided) return;
    setDecided(true);
    for (const block of blocks) onSkip(block.id);
  };

  const heading =
    blocks.length === 1
      ? (parsedById.get(blocks[0]!.id)?.title?.trim() || "Уточнение")
      : `Уточнения · ${blocks.length}`;

  return (
    <div className={`assistant-ask-user-card${decided ? " is-decided" : ""}`}>
      <div className="assistant-ask-user-card-header">
        <span className="assistant-ask-user-card-eyebrow">Ассистент спрашивает</span>
        <div className="assistant-ask-user-card-title">{heading}</div>
      </div>

      {blocks.map((block) => {
        const args = parsedById.get(block.id);
        if (!args) {
          return (
            <div key={block.id} className="assistant-ask-user-card-section">
              <p className="assistant-ask-user-card-error">Не удалось разобрать вопросы.</p>
            </div>
          );
        }
        const showSectionTitle = blocks.length > 1 && args.title?.trim();
        return (
          <div key={block.id} className="assistant-ask-user-card-section">
            {showSectionTitle ? (
              <div className="assistant-ask-user-card-section-title">{args.title!.trim()}</div>
            ) : null}
            <ul className="assistant-ask-user-card-questions">
              {args.questions.map((q, qi) => {
                const picks = selected[block.id]?.[q.id] ?? [];
                const showPrompt = args.questions.length > 1 || q.prompt.trim() !== (args.title?.trim() ?? "");
                return (
                  <li key={q.id} className="assistant-ask-user-card-question">
                    {showPrompt ? (
                      <div className="assistant-ask-user-card-prompt">
                        {args.questions.length > 1 ? (
                          <span className="assistant-ask-user-card-prompt-index">{qi + 1}</span>
                        ) : null}
                        <span>{q.prompt}</span>
                      </div>
                    ) : null}
                    <div
                      className="assistant-ask-user-card-options"
                      role={q.allowMultiple ? "group" : "radiogroup"}
                      aria-label={q.prompt}
                    >
                      {q.options.map((opt) => {
                        const checked = picks.includes(opt.id);
                        return (
                          <label
                            key={opt.id}
                            className={`assistant-ask-user-card-option${checked ? " is-selected" : ""}`}
                          >
                            <input
                              type={q.allowMultiple ? "checkbox" : "radio"}
                              name={`${block.id}-${q.id}`}
                              checked={checked}
                              disabled={decided}
                              onChange={() => toggleOption(block.id, q, opt.id)}
                            />
                            <span className="assistant-ask-user-card-option-mark" aria-hidden />
                            <span className="assistant-ask-user-card-option-label">{opt.label}</span>
                          </label>
                        );
                      })}
                    </div>
                    {q.allowCustom ? (
                      <textarea
                        className="assistant-ask-user-card-custom"
                        rows={2}
                        placeholder="Или свой ответ…"
                        value={customText[block.id]?.[q.id] ?? ""}
                        disabled={decided}
                        onChange={(e) =>
                          setCustomText((prev) => ({
                            ...prev,
                            [block.id]: { ...(prev[block.id] ?? {}), [q.id]: e.target.value },
                          }))
                        }
                      />
                    ) : null}
                  </li>
                );
              })}
            </ul>
          </div>
        );
      })}

      <div className="assistant-ask-user-card-actions">
        <button type="button" className="assistant-btn" disabled={decided} onClick={handleSkip}>
          Пропустить
        </button>
        <button
          type="button"
          className="assistant-btn primary"
          disabled={decided || !allReady}
          onClick={handleSubmit}
        >
          Ответить
        </button>
      </div>
    </div>
  );
}
