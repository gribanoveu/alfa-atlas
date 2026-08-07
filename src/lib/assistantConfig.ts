/** Central place for the assistant chat panel's tunable constants — system
 * prompt, model-picker labels, input sizing, context-bar thresholds.
 * Collected here (instead of scattered across `AssistantPanel.tsx`/
 * `useLlmChat.ts`/`LlmTab.tsx`) so future additions/changes touch one file
 * rather than hunting through components for a magic string or number. */

// Placeholder — expected to evolve once tool-calling/project-context
// injection exist (see AI_HARNESS.md's "not built yet" section).
export const ASSISTANT_SYSTEM_PROMPT =
  "Ты — ассистент, встроенный в Alfa Atlas, редактор технической документации Альфа-Банка. Отвечай кратко, по делу, на русском языке, если пользователь не пишет иначе.";

// Synthetic model-picker value/label for "no explicit pin — resolve the
// first live model at request time" (mirrors `domain::llm::
// LlmProviderConfig.model: null`). Shared by the Settings model dropdown
// and the assistant panel's own picker so both offer the identical choice.
export const AUTO_MODEL_VALUE = "";
export const AUTO_MODEL_LABEL = "Авто (первая доступная)";

// Visible text lines in the chat compose box (fixed, not auto-growing —
// see `AssistantPanel.css`'s `.assistant-chat-input` comment).
export const CHAT_INPUT_ROWS = 4;

// The context-usage bar switches to its warning color once estimated usage
// crosses this fraction of the active model's `limit.context`.
export const CONTEXT_NEAR_LIMIT_RATIO = 0.9;
