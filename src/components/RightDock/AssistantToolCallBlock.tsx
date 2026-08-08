import { AlertCircle, Check, ChevronDown, ChevronRight, File, Folder, Loader2 } from "lucide-react";
import { useState } from "react";
import {
  describeMatchSource,
  describeToolActivity,
  describeToolResult,
  formatToolArguments,
} from "../../lib/assistantConfig";
import type { ToolResult } from "../../lib/aiTools";
import type { ToolCallBlock } from "../../lib/chatBlocks";

/** A file's content can be arbitrarily large — this caps how much of it the
 * expanded detail view renders, purely as a rendering safeguard (the model
 * itself still received the untruncated content; this doesn't change what
 * was sent). */
const MAX_DETAIL_CHARS = 4000;

function truncateForDisplay(text: string): { text: string; truncated: boolean } {
  if (text.length <= MAX_DETAIL_CHARS) return { text, truncated: false };
  return { text: text.slice(0, MAX_DETAIL_CHARS), truncated: true };
}

type AssistantToolCallBlockProps = {
  block: ToolCallBlock;
};

/** One permanent, chronological entry for a single tool invocation inside
 * an assistant message's transcript — a status icon, the "what is/was being
 * done" line (`describeToolActivity`, reused as-is regardless of status),
 * and once settled, a dimmed one-line result summary (`describeToolResult`).
 * Never disappears once appended, unlike the old transient `toolActivity`
 * list it replaces — see `useLlmChat`'s `MessageBlock` model.
 *
 * Clicking the header expands a detail view — the raw arguments plus,
 * once settled, the full result (file content / file list / search
 * matches) or the full error — for inspecting exactly what happened,
 * beyond the one-line summary. Collapsed by default, matching Cursor/
 * Claude Code's own collapsed-by-default tool-call display. */
export function AssistantToolCallBlock({ block }: AssistantToolCallBlockProps) {
  const [expanded, setExpanded] = useState(false);
  const Chevron = expanded ? ChevronDown : ChevronRight;

  return (
    <div className={`assistant-tool-call assistant-tool-call-${block.status}`}>
      <button
        type="button"
        className="assistant-tool-call-header"
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
      >
        <Chevron className="assistant-tool-call-chevron" size={12} aria-hidden />
        {block.status === "running" ? (
          <Loader2 className="assistant-tool-call-icon assistant-chat-tool-spinner" size={13} aria-hidden />
        ) : block.status === "done" ? (
          <Check className="assistant-tool-call-icon" size={13} aria-hidden />
        ) : (
          <AlertCircle className="assistant-tool-call-icon" size={13} aria-hidden />
        )}
        <span className="assistant-tool-call-label">{describeToolActivity(block.name, block.argumentsJson)}</span>
      </button>
    
      {expanded ? (
  <div className="assistant-tool-call-detail">
    {block.status !== "running" ? (
      <div className="assistant-tool-call-detail-summary">
        {describeToolResult(block)}
      </div>
    ) : null}

    <div className="assistant-tool-call-detail-section">
      <div className="assistant-tool-call-detail-label">
        Аргументы
      </div>

      <pre className="assistant-tool-call-detail-code">
        {formatToolArguments(block.argumentsJson)}
      </pre>
    </div>

    {block.status === "done" && block.result ? (
      <ToolResultDetail result={block.result} />
    ) : null}

    {block.status === "error" ? (
      <div className="assistant-tool-call-detail-section">
        <div className="assistant-tool-call-detail-label">
          Ошибка
        </div>

        <pre className="assistant-tool-call-detail-error">
          {block.errorMessage ?? "неизвестная ошибка"}
        </pre>
      </div>
    ) : null}
  </div>
) : null}
    </div>
  );
}

function ToolResultDetail({ result }: { result: ToolResult }) {
  switch (result.tool) {
    case "file": {
      const { text, truncated } = truncateForDisplay(result.result);
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Содержимое файла</div>
          <pre className="assistant-tool-call-detail-code">
            {text}
            {truncated ? "\n… (обрезано)" : ""}
          </pre>
        </div>
      );
    }
    case "fileList":
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Файлы</div>
          {result.result.length === 0 ? (
            <p className="assistant-tool-call-detail-empty">Пусто</p>
          ) : (
            <ul className="assistant-tool-call-detail-list">
              {result.result.map((entry) => (
                <li key={entry.path}>
                  {entry.isDir ? (
                    <Folder className="assistant-tool-call-detail-icon" size={12} aria-hidden />
                  ) : (
                    <File className="assistant-tool-call-detail-icon" size={12} aria-hidden />
                  )}
                  <span>{entry.path}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      );
    case "semanticSearchResults":
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Результаты</div>
          {result.result.length === 0 ? (
            <p className="assistant-tool-call-detail-empty">Ничего не найдено</p>
          ) : (
            <ul className="assistant-tool-call-detail-list assistant-tool-call-detail-matches">
              {result.result.map((match, i) => {
                const source = describeMatchSource(match.source);
                return (
                  <li key={`${match.path}-${match.startByte}-${i}`}>
                    <div className="assistant-tool-call-detail-match-head">
                      <span className="assistant-tool-call-detail-match-path">{match.path}</span>
                      <span
                        className={`assistant-tool-call-source-badge assistant-tool-call-source-${match.source}`}
                        title={source.title}
                      >
                        {source.label}
                      </span>
                    </div>
                    <div className="assistant-tool-call-detail-match-snippet">{match.snippet}</div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      );
    default:
      return null;
  }
}
