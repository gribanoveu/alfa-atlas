import { invoke } from "@tauri-apps/api/core";
import type {
  ArtifactContent,
  ArtifactKind,
  ArtifactRecord,
  ArtifactSummary,
  RenderedArtifact,
} from "./artifacts";
import type { UpdatedReference } from "./project";
import type { PlanTodo } from "./plans";
import type { StandardsReport } from "./standards";
import type { Diagnostic } from "./workspaceIndex";

// Mirrors `domain::ai_access::AiAccessMode` in
// `src-tauri/src/domain/ai_access.rs` (`#[serde(rename_all = "camelCase")]`
// on the enum variants).
export type AiAccessMode = "docsOnly" | "fullRepo";

// Mirrors `domain::conversation_mode::ConversationMode`. Distinct from
// `AiAccessMode` above — this is the chat's behavioral mode (which system
// prompt, which tool subset), not a filesystem-access boundary. Not
// persisted anywhere server-side; the frontend threads it into every
// `llm_chat_stream`/`llm_chat_stream_resume`/`ai_get_tool_definitions` call.
export type ConversationMode = "agent" | "plan" | "question";

/**
 * Mirrors `domain::ai_tools::CheckKind`. `"problems"` = workspace
 * diagnostics (Problems panel). `"standards"` = the API-documentation
 * corporate standard checker (same engine as the Стандарты panel).
 */
export type CheckKind = "problems" | "standards";

export type ToolFileEntry = {
  path: string;
  isDir: boolean;
};

// Mirrors `domain::ai_tools::MatchSource`.
export type MatchSource = "semantic" | "lexical" | "symbol";

// Mirrors `domain::ai_tools::ToolMatch`. `score` is only comparable within
// the same `source`: `"semantic"` is `1 - cosineDistance` (higher is
// better), `"lexical"` is a weighted token-occurrence score, `"symbol"` is
// `1` for an exact name match or `0.9` for a path-segment match.
export type ToolMatch = {
  path: string;
  snippet: string;
  score: number;
  startByte: number;
  endByte: number;
  qualifiedName: string | null;
  source: MatchSource;
};

/** Mirrors `domain::ai_tools::SemanticSearchMeta`. */
export type SemanticSearchMeta = {
  tiersUsed: string[];
  symbolHits: number;
  extractedTokens: string[];
  weak: boolean;
  hint: string | null;
};

/** Mirrors `domain::ai_tools::SemanticSearchPayload`. */
export type SemanticSearchPayload = {
  matches: ToolMatch[];
  meta: SemanticSearchMeta;
};

/**
 * Normalize a `semanticSearchResults` payload — older chats stored a bare
 * `ToolMatch[]`; current wire shape is `{ matches, meta }`.
 */
export function normalizeSemanticSearchResult(
  result: ToolMatch[] | SemanticSearchPayload,
): SemanticSearchPayload {
  if (Array.isArray(result)) {
    const counts = new Map<MatchSource, number>();
    for (const m of result) counts.set(m.source, (counts.get(m.source) ?? 0) + 1);
    const symbolHits = counts.get("symbol") ?? 0;
    const onlyLexical =
      result.length > 0 &&
      result.every((m) => m.source === "lexical") &&
      symbolHits === 0;
    const weak = result.length === 0 || onlyLexical;
    return {
      matches: result,
      meta: {
        tiersUsed: [...counts.keys()],
        symbolHits,
        extractedTokens: [],
        weak,
        hint: weak
          ? result.length === 0
            ? "Ничего не найдено. Добавьте английские имена методов/классов (camelCase) и повторите поиск."
            : "Поиск шёл по тексту без совпадений по именам. Уточните query английскими терминами или дождитесь синхронизации эмбеддингов."
          : null,
      },
    };
  }
  return result;
}

// Mirrors the Rust `ToolCall`/`ToolResult` enums in
// `src-tauri/src/domain/ai_tools.rs` (adjacently tagged:
// `#[serde(tag = "tool", content = "args" | "result")]`).
// One `{old, new}` search-and-replace pair within an `editFile` call —
// `old` must match the target file's current content exactly once, or the
// whole call is rejected (see `domain::ai_tools::FileEdit`).
export type FileEdit = { old: string; new: string };

// Mirrors `domain::ai_tools::TodoStatus`.
export type TodoStatus = "pending" | "inProgress" | "completed" | "cancelled";

// Mirrors `domain::ai_tools::Task`. Never persisted server-side — owned by
// `useLlmChat`'s `todoListRef` between turns, round-tripped through
// `ChatStreamOutcome`/`PendingApproval` exactly like `history` already is.
export type Task = {
  id: string;
  title: string;
  status: TodoStatus;
  note: string | null;
};

export type ToolCall =
  | { tool: "readFile"; args: { path: string; startLine: number | null; endLine: number | null } }
  | { tool: "listFiles"; args: { path: string | null; depth: number | null; pattern: string | null } }
  | { tool: "semanticSearch"; args: { query: string; topK: number | null } }
  | {
      tool: "grep";
      args: {
        pattern: string;
        path: string | null;
        glob: string | null;
        caseInsensitive: boolean | null;
        maxResults: number | null;
      };
    }
  | { tool: "gitDiff"; args: { path: string; scope: string | null; commit: string | null } }
  | { tool: "gitBlame"; args: { path: string; startLine: number | null; endLine: number | null } }
  | { tool: "check"; args: { kind: CheckKind; path: string | null } }
  | { tool: "writeFile"; args: { path: string; content: string } }
  | { tool: "editFile"; args: { path: string; edits: FileEdit[] } }
  | { tool: "deleteFile"; args: { path: string } }
  | { tool: "createDirectory"; args: { path: string; template: string | null } }
  | { tool: "deleteDirectory"; args: { path: string; recursive: boolean | null } }
  | { tool: "move"; args: { path: string; newPath: string } }
  | { tool: "requestFullRepoAccess"; args: { reason: string } }
  | { tool: "requestModeSwitch"; args: { mode: ConversationMode; reason: string } }
  | { tool: "getAsciidocTemplates"; args: { ids: string[] } }
  | {
      tool: "askUser";
      args: {
        title: string | null;
        questions: Array<{
          id: string;
          prompt: string;
          options: Array<{ id: string; label: string }>;
          allowMultiple: boolean;
        }>;
      };
    }
  | {
      tool: "requestArtifact";
      args: { kind: ArtifactKind; title: string; purpose: string; prefill: ArtifactContent | null };
    }
  | { tool: "artifactList"; args: Record<string, never> }
  | { tool: "artifactRead"; args: { id: string } }
  | { tool: "todoWrite"; args: { titles: string[] } }
  | { tool: "todoUpdate"; args: { id: string; status: "completed" | "cancelled"; note: string | null } }
  | {
      tool: "memory";
      args: {
        op: "wake" | "note" | "nap" | "recall" | "zoom" | "forget" | "config";
        scope: "project" | "global";
        text: string | null;
        pattern: string | null;
        block: string | null;
        knob: string | null;
        part: number | null;
        snapshotT: number | null;
      };
    }
  | {
      tool: "createPlan";
      args: {
        name: string;
        overview: string;
        plan: string;
        todos: Array<{ id: string; content: string }>;
      };
    }
  | {
      tool: "updatePlan";
      args: {
        planId: string;
        name: string | null;
        overview: string | null;
        plan: string | null;
        todos: Array<{ id: string; content: string }> | null;
      };
    }
  | { tool: "readPlan"; args: { planId: string } }
  | {
      tool: "updatePlanTodo";
      args: {
        planId: string;
        id: string;
        status: "completed" | "cancelled";
        note: string | null;
      };
    };

// Mirrors Rust's `domain::ai_tools::FileDiffStats` — attached to a settled
// `fileWritten`/`fileEdited`/`fileDeleted` result, computed once
// server-side (`services::text_diff::diff_stats`) and consumed both by the
// chat UI (a `+N -M` badge and colored diff view) and by the model itself
// (the same `ToolResult` JSON is what it reads back). `linesAdded`/
// `linesRemoved` are always the true, untruncated totals even when
// `unifiedDiff` was cut short (`truncated`).
export type FileDiffStats = {
  linesAdded: number;
  linesRemoved: number;
  unifiedDiff: string;
  truncated: boolean;
};

/** Contiguous authorship run from `gitBlame` — mirrors
 * `domain::git::GitBlameHunk`. */
export type GitBlameHunk = {
  startLine: number;
  endLine: number;
  commit: string;
  author: string;
  authoredAt: string;
  summary: string;
};

/** One line hit from `grep` — mirrors `domain::ai_tools::GrepMatch`. */
export type GrepMatch = {
  path: string;
  line: number;
  text: string;
};

/** One resolved template from `getAsciidocTemplates` — mirrors
 * `domain::ai_tools::AsciidocTemplateEntry`. */
export type AsciidocTemplateEntry = {
  id: string;
  label: string;
  category: string;
  template: string;
};

// `ToolResult`'s `"file"` case carries range/total-line metadata alongside
// the content — 1-indexed, inclusive, the range actually returned (after
// clamping), not necessarily what was requested. `0`/`0`/`0` on an empty
// file (there is no line 1 to claim).
export type ToolResult =
  | { tool: "file"; result: { content: string; startLine: number; endLine: number; totalLines: number } }
  | { tool: "fileList"; result: ToolFileEntry[] }
  | { tool: "semanticSearchResults"; result: ToolMatch[] | SemanticSearchPayload }
  | { tool: "grepResults"; result: { matches: GrepMatch[]; truncated: boolean } }
  | { tool: "gitDiff"; result: { path: string; label: string; diff: FileDiffStats; isBinary: boolean } }
  | { tool: "gitBlame"; result: { path: string; hunks: GitBlameHunk[]; truncated: boolean } }
  | { tool: "checkResults"; result: { kind: CheckKind; diagnostics: Diagnostic[]; truncated: boolean } }
  | { tool: "standardsChecked"; result: { report: StandardsReport; truncated: boolean } }
  | { tool: "fileWritten"; result: { path: string; diff: FileDiffStats } }
  | { tool: "fileEdited"; result: { path: string; diff: FileDiffStats } }
  | { tool: "fileDeleted"; result: { path: string; diff: FileDiffStats } }
  | { tool: "directoryCreated"; result: { path: string; template: string | null; createdFiles: string[] } }
  | { tool: "directoryDeleted"; result: { path: string } }
  | { tool: "moved"; result: { from: string; to: string; updatedFiles: UpdatedReference[] } }
  | { tool: "accessModeChanged"; result: { mode: AiAccessMode } }
  | { tool: "modeSwitchRequested"; result: { mode: ConversationMode; reason: string } }
  | {
      tool: "asciidocTemplates";
      result: { templates: AsciidocTemplateEntry[]; notFound: string[] };
    }
  | {
      tool: "skillSearch";
      result: {
        matches: Array<{ name: string; description: string; source: "bundled" | "user" }>;
      };
    }
  | {
      tool: "skillLoaded";
      result: { name: string; source: "bundled" | "user"; body: string; files: string[] };
    }
  | { tool: "skillFile"; result: { name: string; path: string; content: string } }
  | { tool: "todoWritten"; result: Task[] }
  | { tool: "todoUpdated"; result: Task[] }
  | { tool: "memory"; result: { text: string } }
  | {
      tool: "askUser";
      result: {
        answers: Array<{
          questionId: string;
          selectedOptionIds: string[];
          selectedLabels: string[];
          customText: string | null;
        }>;
      };
    }
  // Settled `requestArtifact` (resolved from the user's decision on resume)
  // and settled `artifact` op "read" share this shape — which of the two it
  // was is already on the tool-call block's `name`.
  | { tool: "artifact"; result: { artifact: ArtifactRecord; rendered: RenderedArtifact } }
  | { tool: "artifactList"; result: { artifacts: ArtifactSummary[] } }
  | {
      tool: "planCreated";
      result: {
        planId: string;
        name: string;
        overview: string;
        todoCount: number;
        todos: PlanTodo[];
      };
    }
  | {
      tool: "planUpdated";
      result: {
        planId: string;
        name: string;
        overview: string;
        todoCount: number;
        todos: PlanTodo[];
      };
    }
  | {
      tool: "planRead";
      result: {
        planId: string;
        name: string;
        overview: string;
        plan: string;
        todos: PlanTodo[];
      };
    }
  | {
      tool: "planTodoUpdated";
      result: { planId: string; todos: PlanTodo[] };
    };

/**
 * Runs one AI-harness tool call against whichever project is currently
 * open. The caller never passes a docs/repo root or access mode — the
 * backend resolves the current project and its configured `AiAccessMode`/
 * tool allowlist itself (`services::ai_tools::current_scope` in Rust).
 */
export function aiExecuteTool(call: ToolCall, todos: Task[] = []): Promise<ToolResult> {
  return invoke<ToolResult>("ai_execute_tool", { call, todos });
}

/** Which part of the filesystem the harness (and `embedding_sync`) may see
 * for the currently open project — "docsOnly" (default) or "fullRepo". */
export function getAiAccessMode(): Promise<AiAccessMode> {
  return invoke<AiAccessMode>("ai_get_access_mode");
}

export function setAiAccessMode(mode: AiAccessMode): Promise<void> {
  return invoke("ai_set_access_mode", { mode });
}

/** Tool names (e.g. `"writeFile"`) the currently open project has persisted
 * as "always allow" via an approval card's "Разрешать всегда" button —
 * loaded once when an assistant chat panel mounts so a choice made in one
 * chat carries into every later chat on this repo. */
export function getAutoApprovedTools(): Promise<string[]> {
  return invoke<string[]>("ai_get_auto_approved_tools");
}

type AutoApprovedToolsChange = { tool: string; autoApproved: boolean };

const autoApprovedToolsListeners = new Set<(change: AutoApprovedToolsChange) => void>();

/** Live-subscribes to every successful `setToolAutoApproved` call, from
 * *any* caller — an approval card's "Разрешать всегда", or a revoke in
 * `PermissionsTab`. `useLlmChat`'s `trustedToolsRef` is only ever loaded
 * once per chat-panel mount (see its own doc comment); without this, a
 * revoke made in Settings while a chat panel is already open would never
 * reach that panel's in-memory trust set, and it would keep silently
 * auto-approving the "revoked" tool for the rest of its mounted lifetime.
 * Returns an unsubscribe function. */
export function onAutoApprovedToolsChange(listener: (change: AutoApprovedToolsChange) => void): () => void {
  autoApprovedToolsListeners.add(listener);
  return () => autoApprovedToolsListeners.delete(listener);
}

/** Persists (or revokes) one tool's "always allow" status for the currently
 * open project, then notifies every `onAutoApprovedToolsChange` listener —
 * see that function's doc comment for why the notification matters. */
export async function setToolAutoApproved(tool: string, autoApproved: boolean): Promise<void> {
  await invoke("ai_set_tool_auto_approved", { tool, autoApproved });
  for (const listener of autoApprovedToolsListeners) listener({ tool, autoApproved });
}

/** Tool names the currently open project actually allows right now — the
 * customized `ai_allowed_tools` set if one was ever saved, else the current
 * access mode's default (which is every tool today). */
export function getAllowedTools(): Promise<string[]> {
  return invoke<string[]>("ai_get_allowed_tools");
}

/** Every tool shown in Settings → Permissions — mirrors
 * `services::ai_tools::permission_tool_catalog` on the Rust side. */
export function listPermissionTools(): Promise<string[]> {
  return invoke<string[]>("ai_list_permission_tools");
}

/** Persists (or revokes) one tool's membership in `ai_allowed_tools` for the
 * currently open project. */
export function setToolAllowed(tool: string, allowed: boolean): Promise<void> {
  return invoke("ai_set_tool_allowed", { tool, allowed });
}

/** Combined OptMem wake for project + global stores (injected at chat start). */
export function getMemoryWake(): Promise<string> {
  return invoke<string>("ai_get_memory_wake");
}

// Mirrors `domain::llm::LlmToolDefinition` in `src-tauri/src/domain/llm.rs`
// (`#[serde(rename_all = "camelCase")]`) — the same definitions actually
// advertised to the model for function-calling.
export type LlmToolDefinition = {
  name: string;
  description: string;
  parameters: unknown;
};

/** The tools currently allowed for the open project's persisted access
 * mode/allowlist, intersected with `conversationMode`'s own tool subset —
 * the same source `llm_chat_stream` uses for real function-calling
 * (`services::ai_tools::llm_tool_definitions`). */
export function getToolDefinitions(conversationMode: ConversationMode): Promise<LlmToolDefinition[]> {
  return invoke<LlmToolDefinition[]>("ai_get_tool_definitions", { conversationMode });
}
