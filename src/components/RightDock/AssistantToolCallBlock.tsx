import { AlertCircle, Bot, Check, ChevronDown, ChevronRight, Clock, File, Folder, Loader2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import {
  basename,
  describeMatchSource,
  describeToolActivity,
  describeToolResult,
  formatToolArguments,
  TOOL_APPROVAL_TIMEOUT_MS,
} from "../../lib/assistantConfig";
import type { FileDiffStats, FileEdit, Task, ToolResult, TodoStatus } from "../../lib/aiTools";
import type { ToolCallBlock } from "../../lib/chatBlocks";
import { DeleteDirectoryReview } from "./DeleteDirectoryReview";
import { DeleteFileReview } from "./DeleteFileReview";
import { EditFileDiffReview } from "./EditFileDiffReview";
import { WriteFileDiffReview } from "./WriteFileDiffReview";

/** A file's content can be arbitrarily large — this caps how much of it the
 * expanded detail view renders, purely as a rendering safeguard (the model
 * itself still received the untruncated content; this doesn't change what
 * was sent). */
const MAX_DETAIL_CHARS = 4000;

function truncateForDisplay(text: string): { text: string; truncated: boolean } {
  if (text.length <= MAX_DETAIL_CHARS) return { text, truncated: false };
  return { text: text.slice(0, MAX_DETAIL_CHARS), truncated: true };
}

function parseArgs(argumentsJson: string): Record<string, unknown> {
  try {
    const parsed: unknown = JSON.parse(argumentsJson);
    return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

/** Docs-relative paths the REST-method folder scaffold will create — mirrors
 * `services::ai_tools::rest_endpoint_created_files` / `docs_fs::create_rest_endpoint_folder`. */
function restEndpointPreviewFiles(folderPath: string): string[] {
  const parts = folderPath.split("/").filter(Boolean);
  const methodName = parts.length > 0 ? parts[parts.length - 1]! : folderPath;
  const child = (name: string) =>
    folderPath === "" || folderPath === "." ? name : `${folderPath}/${name}`;
  return [
    child(`${methodName}.adoc`),
    child("request.adoc"),
    child("response.adoc"),
    child(`${methodName}.puml`),
  ];
}

/** Guards a pending `editFile` call's `args.edits` before handing it to
 * `EditFileDiffReview` — `parseArgs` only guarantees valid JSON, not this
 * shape. */
function isFileEditArray(value: unknown): value is FileEdit[] {
  return (
    Array.isArray(value) &&
    value.every(
      (e) =>
        e !== null &&
        typeof e === "object" &&
        typeof (e as FileEdit).old === "string" &&
        typeof (e as FileEdit).new === "string",
    )
  );
}

/** `diff` for a settled `writeFile`/`editFile`/`deleteFile` call, or `null`
 * for every other tool/status — the one spot both the header badge and the
 * expanded detail view read from, so they never drift on which tools carry
 * a diff. */
function diffStatsFor(block: ToolCallBlock): FileDiffStats | null {
  if (block.status !== "done" || !block.result) return null;
  switch (block.result.tool) {
    case "fileWritten":
    case "fileEdited":
    case "fileDeleted":
      return block.result.result.diff;
    case "gitDiff":
      return block.result.result.isBinary ? null : block.result.result.diff;
    default:
      return null;
  }
}

/** The always-visible `+N -M` pill in a settled tool call's header —
 * doesn't require expanding the block, matching how a git-status line or a
 * PR file list shows change size at a glance. Omits whichever side is zero
 * (a brand-new file shows only `+N`, a delete only `-M`), and renders
 * nothing at all when nothing actually changed (e.g. an overwrite with
 * identical content). */
function DiffBadge({ diff }: { diff: FileDiffStats }) {
  if (diff.linesAdded === 0 && diff.linesRemoved === 0) return null;
  return (
    <span className="assistant-tool-call-diff-badge">
      {diff.linesAdded > 0 ? <span className="assistant-tool-call-diff-added">+{diff.linesAdded}</span> : null}
      {diff.linesRemoved > 0 ? (
        <span className="assistant-tool-call-diff-removed">−{diff.linesRemoved}</span>
      ) : null}
    </span>
  );
}

/** Lightweight colored line view of `diff.unifiedDiff` for the expanded
 * detail — deliberately not the Monaco `DiffEditor` `WriteFileDiffReview`
 * uses for the live pending-approval preview: that's a heavy editor
 * instance meant for one active review, while this renders inline for
 * every settled call in a potentially long chat history. Classifies each
 * line by its unified-diff leading character; `similar`'s
 * `unified_diff()` output has no `---`/`+++` file header (never
 * requested), only `@@ ... @@` hunk markers and `+`/`-`/` `-prefixed
 * lines. */
function DiffLines({ diff }: { diff: FileDiffStats }) {
  if (diff.unifiedDiff === "") return null;
  const lines = diff.unifiedDiff.split("\n").filter((_, i, arr) => !(i === arr.length - 1 && arr[i] === ""));
  return (
    <div className="assistant-tool-call-detail-section">
      <div className="assistant-tool-call-detail-label">Изменения</div>
      <div className="assistant-tool-call-diff">
        {lines.map((line, i) => {
          const kind = line.startsWith("+")
            ? "added"
            : line.startsWith("-")
              ? "removed"
              : line.startsWith("@@")
                ? "hunk"
                : "context";
          return (
            <div key={i} className={`assistant-tool-call-diff-line assistant-tool-call-diff-line-${kind}`}>
              {line}
            </div>
          );
        })}
        {diff.truncated ? <div className="assistant-tool-call-diff-truncated">… диф обрезан</div> : null}
      </div>
    </div>
  );
}

type AssistantToolCallBlockProps = {
  block: ToolCallBlock;
  /** The open project's docs root — needed by the `writeFile` approval
   * card's diff preview to fetch the file's current content. */
  docsRoot: string;
  /** Called from a `"pendingApproval"` card's Approve/Deny buttons — see
   * `useLlmChat`'s `decideToolCall`. Unused for every other block status. */
  onDecide: (id: string, approved: boolean, trust: boolean) => void;
};

/** One permanent, chronological entry for a single tool invocation inside
 * an assistant message's transcript — a status icon, the "what is/was being
 * done" line (`describeToolActivity`, reused as-is regardless of status),
 * and once settled, a dimmed one-line result summary (`describeToolResult`).
 * Never disappears once appended, unlike the old transient `toolActivity`
 * list it replaces — see `useLlmChat`'s `MessageBlock` model.
 *
 * A `"pendingApproval"` block (`writeFile`/`requestFullRepoAccess` awaiting
 * a decision) additionally renders `ToolApprovalCard` right below the
 * header, always visible regardless of `expanded` — the user needs to see
 * and act on it immediately, not hunt for a collapsed row. It disappears on
 * its own once the block transitions away from `"pendingApproval"` (the
 * real `TOOL_CALL_EVENT`, fired once the round actually resumes with a
 * decision — manual or timed out — for every call in it).
 *
 * Clicking the header expands a detail view — the raw arguments plus,
 * once settled, the full result (file content / file list / search
 * matches) or the full error — for inspecting exactly what happened,
 * beyond the one-line summary. Collapsed by default, matching Cursor/
 * Claude Code's own collapsed-by-default tool-call display. */
export function AssistantToolCallBlock({ block, docsRoot, onDecide }: AssistantToolCallBlockProps) {
  const [expanded, setExpanded] = useState(false);
  const Chevron = expanded ? ChevronDown : ChevronRight;
  const diff = diffStatsFor(block);
  const settled = block.status !== "running" && block.status !== "pendingApproval";
  const summary = settled ? describeToolResult(block) : "";

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
        ) : block.status === "pendingApproval" ? (
          <Clock className="assistant-tool-call-icon" size={13} aria-hidden />
        ) : (
          <AlertCircle className="assistant-tool-call-icon" size={13} aria-hidden />
        )}
        <span className="assistant-tool-call-label">{describeToolActivity(block.name, block.argumentsJson)}</span>
        {diff ? <DiffBadge diff={diff} /> : null}
        {block.autoApproved ? (
          <span
            className="assistant-tool-call-auto-approved"
            title="Одобрено автоматически — вы отключили запрос подтверждения для этого действия в этом диалоге"
          >
            <Bot size={12} aria-hidden />
          </span>
        ) : null}
      </button>

      {summary ? <div className="assistant-tool-call-summary">{summary}</div> : null}

      {block.status === "pendingApproval" ? (
        <ToolApprovalCard block={block} docsRoot={docsRoot} onDecide={onDecide} />
      ) : null}

      {expanded ? (
        <div className="assistant-tool-call-detail">
          {summary ? <div className="assistant-tool-call-detail-summary">{summary}</div> : null}

          <div className="assistant-tool-call-detail-section">
            <div className="assistant-tool-call-detail-label">Аргументы</div>

            <pre className="assistant-tool-call-detail-code">{formatToolArguments(block.argumentsJson)}</pre>
          </div>

          {block.status === "done" && block.result ? <ToolResultDetail result={block.result} /> : null}

          {block.status === "error" ? (
            <div className="assistant-tool-call-detail-section">
              <div className="assistant-tool-call-detail-label">Ошибка</div>

              <pre className="assistant-tool-call-detail-error">{block.errorMessage ?? "неизвестная ошибка"}</pre>
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

/** Body of a `"pendingApproval"` card: whatever context helps the user
 * judge the action (a diff for `writeFile`, the stated reason for
 * `requestFullRepoAccess`), the countdown strip, a "don't ask again" button
 * that persists per-tool, per-project (`useLlmChat`'s `getAutoApprovedTools`/
 * `setToolAutoApproved`), and Approve/Deny. Disables itself the instant
 * either button is clicked — `useLlmChat`'s own timeout can still beat a
 * slow click to the punch, but this at least prevents a double decision
 * from this card itself. */
function ToolApprovalCard({
  block,
  docsRoot,
  onDecide,
}: {
  block: ToolCallBlock;
  docsRoot: string;
  onDecide: (id: string, approved: boolean, trust: boolean) => void;
}) {
  const [decided, setDecided] = useState(false);
  const [showDiff, setShowDiff] = useState(false);
  const args = useMemo(() => parseArgs(block.argumentsJson), [block.argumentsJson]);

  const handleDecide = (approved: boolean, trust = false) => {
    if (decided) return;
    setDecided(true);
    onDecide(block.id, approved, trust);
  };

  return (
    <div className="assistant-tool-approval-card">
      {block.name === "writeFile" && typeof args.path === "string" && typeof args.content === "string" ? (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Файл</div>
          <button
            type="button"
            className="assistant-tool-approval-path assistant-tool-approval-path-toggle"
            aria-expanded={showDiff}
            title={args.path}
            onClick={() => setShowDiff((v) => !v)}
          >
            {showDiff ? (
              <ChevronDown className="assistant-tool-call-chevron" size={12} aria-hidden />
            ) : (
              <ChevronRight className="assistant-tool-call-chevron" size={12} aria-hidden />
            )}
            <span>{basename(args.path)}</span>
          </button>
          {showDiff ? <WriteFileDiffReview docsRoot={docsRoot} path={args.path} content={args.content} /> : null}
        </div>
      ) : block.name === "editFile" && typeof args.path === "string" && isFileEditArray(args.edits) ? (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Файл</div>
          <button
            type="button"
            className="assistant-tool-approval-path assistant-tool-approval-path-toggle"
            aria-expanded={showDiff}
            title={args.path}
            onClick={() => setShowDiff((v) => !v)}
          >
            {showDiff ? (
              <ChevronDown className="assistant-tool-call-chevron" size={12} aria-hidden />
            ) : (
              <ChevronRight className="assistant-tool-call-chevron" size={12} aria-hidden />
            )}
            <span>{basename(args.path)}</span>
            <span className="assistant-tool-approval-edit-count">Правок: {args.edits.length}</span>
          </button>
          {showDiff ? <EditFileDiffReview docsRoot={docsRoot} path={args.path} edits={args.edits} /> : null}
        </div>
      ) : block.name === "createDirectory" && typeof args.path === "string" ? (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Папка</div>
          <div className="assistant-tool-approval-path" title={args.path}>
            {basename(args.path)}
          </div>
          {args.template === "restEndpoint" ? (
            <>
              <div className="assistant-tool-call-detail-label">Шаблон</div>
              <div className="assistant-tool-approval-path">Документация на REST метод</div>
              <div className="assistant-tool-call-detail-label">Будут созданы файлы</div>
              <ul className="assistant-tool-call-detail-list">
                {restEndpointPreviewFiles(args.path).map((file) => (
                  <li key={file} title={file}>
                    <File className="assistant-tool-call-detail-icon" size={12} aria-hidden />
                    <span>{basename(file)}</span>
                  </li>
                ))}
              </ul>
            </>
          ) : null}
        </div>
      ) : block.name === "deleteFile" && typeof args.path === "string" ? (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Файл будет удалён</div>
          <button
            type="button"
            className="assistant-tool-approval-path assistant-tool-approval-path-toggle"
            aria-expanded={showDiff}
            title={args.path}
            onClick={() => setShowDiff((v) => !v)}
          >
            {showDiff ? (
              <ChevronDown className="assistant-tool-call-chevron" size={12} aria-hidden />
            ) : (
              <ChevronRight className="assistant-tool-call-chevron" size={12} aria-hidden />
            )}
            <span>{basename(args.path)}</span>
          </button>
          {showDiff ? <DeleteFileReview docsRoot={docsRoot} path={args.path} /> : null}
        </div>
      ) : block.name === "deleteDirectory" && typeof args.path === "string" ? (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Папка будет удалена</div>
          <button
            type="button"
            className="assistant-tool-approval-path assistant-tool-approval-path-toggle"
            aria-expanded={showDiff}
            title={args.path}
            onClick={() => setShowDiff((v) => !v)}
          >
            {showDiff ? (
              <ChevronDown className="assistant-tool-call-chevron" size={12} aria-hidden />
            ) : (
              <ChevronRight className="assistant-tool-call-chevron" size={12} aria-hidden />
            )}
            <span>{basename(args.path)}</span>
          </button>
          {showDiff ? <DeleteDirectoryReview docsRoot={docsRoot} path={args.path} /> : null}
        </div>
      ) : block.name === "move" && typeof args.path === "string" && typeof args.newPath === "string" ? (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Перемещение</div>
          <div className="assistant-tool-approval-path" title={`${args.path} → ${args.newPath}`}>
            {basename(args.path)} → {basename(args.newPath)}
          </div>
          <div className="assistant-tool-approval-reason">
            Ссылки на файл в других документах будут обновлены автоматически.
          </div>
        </div>
      ) : block.name === "requestFullRepoAccess" && typeof args.reason === "string" ? (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Причина</div>
          <div className="assistant-tool-approval-reason">{args.reason}</div>
        </div>
      ) : block.name === "memory" && args.op === "note" && typeof args.text === "string" ? (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">
            Новая запись в памяти
            {args.scope === "global" ? " (глобальная)" : args.scope === "project" ? " (проектная)" : ""}
          </div>
          <div className="assistant-tool-approval-reason">{args.text}</div>
        </div>
      ) : block.name === "memory" && args.op === "forget" ? (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">
            Забыть summary
            {args.scope === "global" ? " (глобальная)" : args.scope === "project" ? " (проектная)" : ""}
          </div>
          <div className="assistant-tool-approval-reason">
            {typeof args.block === "string" ? `Блок ${args.block}` : "Блок не указан"}
          </div>
        </div>
      ) : block.name === "memory" && args.op === "config" ? (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">
            Настройка памяти
            {args.scope === "global" ? " (глобальная)" : args.scope === "project" ? " (проектная)" : ""}
          </div>
          <div className="assistant-tool-approval-reason">
            {typeof args.knob === "string" && args.knob.trim()
              ? args.knob
              : "Изменение размеров памяти"}
          </div>
        </div>
      ) : null}

      <div className="assistant-tool-approval-actions">
        <div className="assistant-tool-approval-buttons">
          <button type="button" className="assistant-btn" disabled={decided} onClick={() => handleDecide(false)}>
            Отклонить
          </button>
          <button
            type="button"
            className="assistant-btn"
            disabled={decided}
            title="Больше не спрашивать для этого инструмента в этом проекте"
            onClick={() => handleDecide(true, true)}
          >
            Разрешать всегда
          </button>
          <button
            type="button"
            className="assistant-btn primary"
            disabled={decided}
            onClick={() => handleDecide(true)}
          >
            Одобрить
          </button>
        </div>
      </div>

      <ApprovalCountdown deadlineAt={block.deadlineAt} />
    </div>
  );
}

/** A strip that visually depletes from full to empty over the time
 * remaining until `deadlineAt` (`useLlmChat`'s `TOOL_APPROVAL_TIMEOUT_MS`
 * auto-deny deadline) — a single CSS `width` transition kicked off one
 * frame after mount, not a per-frame JS timer, so it stays smooth
 * regardless of render cadence. Purely visual: the actual auto-deny is a
 * real `setTimeout` in `useLlmChat`, independent of whether this component
 * is even mounted to show it. */
function ApprovalCountdown({ deadlineAt }: { deadlineAt?: number }) {
  const [durationMs] = useState(() =>
    deadlineAt !== undefined ? Math.max(0, deadlineAt - Date.now()) : TOOL_APPROVAL_TIMEOUT_MS,
  );
  const [depleted, setDepleted] = useState(false);

  useEffect(() => {
    const raf = requestAnimationFrame(() => setDepleted(true));
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <div className="assistant-tool-approval-timer" aria-hidden="true">
      <div
        className="assistant-tool-approval-timer-fill"
        style={{ transitionDuration: `${durationMs}ms`, width: depleted ? "0%" : "100%" }}
      />
    </div>
  );
}

function ToolResultDetail({ result }: { result: ToolResult }) {
  switch (result.tool) {
    case "file": {
      const { content, startLine, endLine, totalLines } = result.result;
      const { text, truncated } = truncateForDisplay(content);
      const label =
        startLine === 1 && endLine === totalLines
          ? "Содержимое файла"
          : `Содержимое файла (строки ${startLine}–${endLine} из ${totalLines})`;
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">{label}</div>
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
    case "fileWritten":
      return (
        <>
          <div className="assistant-tool-call-detail-section">
            <div className="assistant-tool-call-detail-label">Файл записан</div>
            <pre className="assistant-tool-call-detail-code">{result.result.path}</pre>
          </div>
          <DiffLines diff={result.result.diff} />
        </>
      );
    case "fileEdited":
      return (
        <>
          <div className="assistant-tool-call-detail-section">
            <div className="assistant-tool-call-detail-label">Файл изменён</div>
            <pre className="assistant-tool-call-detail-code">{result.result.path}</pre>
          </div>
          <DiffLines diff={result.result.diff} />
        </>
      );
    case "fileDeleted":
      return (
        <>
          <div className="assistant-tool-call-detail-section">
            <div className="assistant-tool-call-detail-label">Файл удалён</div>
            <pre className="assistant-tool-call-detail-code">{result.result.path}</pre>
          </div>
          <DiffLines diff={result.result.diff} />
        </>
      );
    case "directoryCreated": {
      const { path, template, createdFiles } = result.result;
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Папка создана</div>
          <pre className="assistant-tool-call-detail-code">{path}</pre>
          {template === "restEndpoint" ? (
            <>
              <div className="assistant-tool-call-detail-label">Шаблон</div>
              <pre className="assistant-tool-call-detail-code">Документация на REST метод</pre>
            </>
          ) : null}
          {createdFiles.length > 0 ? (
            <>
              <div className="assistant-tool-call-detail-label">Созданные файлы</div>
              <ul className="assistant-tool-call-detail-list">
                {createdFiles.map((file) => (
                  <li key={file}>
                    <File className="assistant-tool-call-detail-icon" size={12} aria-hidden />
                    <span>{file}</span>
                  </li>
                ))}
              </ul>
            </>
          ) : null}
        </div>
      );
    }
    case "directoryDeleted":
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Папка удалена</div>
          <pre className="assistant-tool-call-detail-code">{result.result.path}</pre>
        </div>
      );
    case "moved": {
      const { from, to, updatedFiles } = result.result;
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Перемещено</div>
          <pre className="assistant-tool-call-detail-code">
            {from} → {to}
          </pre>
          {updatedFiles.length > 0 && (
            <>
              <div className="assistant-tool-call-detail-label">
                Обновлены ссылки в других файлах
              </div>
              <ul className="assistant-tool-call-detail-list">
                {updatedFiles.map((f) => (
                  <li key={f.docsRelativePath}>
                    <File className="assistant-tool-call-detail-icon" size={12} aria-hidden />
                    <span>
                      {f.docsRelativePath} ({f.count})
                    </span>
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      );
    }
    case "accessModeChanged":
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Новый режим доступа</div>
          <pre className="assistant-tool-call-detail-code">
            {result.result.mode === "fullRepo" ? "Весь репозиторий" : "Только документация"}
          </pre>
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
    case "grepResults": {
      const { matches, truncated } = result.result;
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Grep</div>
          {matches.length === 0 ? (
            <p className="assistant-tool-call-detail-empty">Ничего не найдено</p>
          ) : (
            <ul className="assistant-tool-call-detail-list">
              {matches.map((m, i) => (
                <li key={`${m.path}:${m.line}:${i}`}>
                  <span>
                    {m.path}:{m.line} · {m.text}
                  </span>
                </li>
              ))}
            </ul>
          )}
          {truncated ? <p className="assistant-tool-call-detail-empty">… результаты обрезаны</p> : null}
        </div>
      );
    }
    case "checkResults": {
      const { kind, diagnostics, truncated } = result.result;
      const shown = diagnostics.slice(0, 40);
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">
            Проверка ({kind === "problems" ? "проблемы" : kind})
          </div>
          {diagnostics.length === 0 ? (
            <p className="assistant-tool-call-detail-empty">Проблем нет</p>
          ) : (
            <ul className="assistant-tool-call-detail-list">
              {shown.map((d, i) => (
                <li key={`${d.document}:${d.line}:${d.column}:${i}`}>
                  <span>
                    [{d.severity}] {d.document}:{d.line} · {d.message}
                  </span>
                </li>
              ))}
            </ul>
          )}
          {diagnostics.length > shown.length ? (
            <p className="assistant-tool-call-detail-empty">
              … ещё {diagnostics.length - shown.length}
            </p>
          ) : null}
          {truncated ? <p className="assistant-tool-call-detail-empty">… результаты обрезаны</p> : null}
        </div>
      );
    }
    case "standardsChecked": {
      const { report, truncated } = result.result;
      const shown = report.folders.slice(0, 40);
      const passedCount = report.folders.filter((f) => f.passed).length;
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">
            Стандарты документации ({passedCount}/{report.folders.length} папок соответствуют)
          </div>
          {report.folders.length === 0 ? (
            <p className="assistant-tool-call-detail-empty">Папки с документацией не найдены</p>
          ) : (
            <ul className="assistant-tool-call-detail-list">
              {shown.map((f) => {
                const pct = f.maxScore > 0 ? Math.round((f.score / f.maxScore) * 100) : 0;
                const failing = f.findings.filter((finding) => !finding.passed);
                return (
                  <li key={f.folder}>
                    <span>
                      [{f.passed ? "✓" : "✗"} {pct}%] {f.folder}
                      {failing.length > 0
                        ? ` — ${failing.map((finding) => `${finding.ruleId}: ${finding.message}`).join("; ")}`
                        : ""}
                    </span>
                  </li>
                );
              })}
            </ul>
          )}
          {report.folders.length > shown.length ? (
            <p className="assistant-tool-call-detail-empty">
              … ещё {report.folders.length - shown.length}
            </p>
          ) : null}
          {truncated ? <p className="assistant-tool-call-detail-empty">… результаты обрезаны</p> : null}
        </div>
      );
    }
    case "gitDiff":
      return (
        <>
          <div className="assistant-tool-call-detail-section">
            <div className="assistant-tool-call-detail-label">Git diff</div>
            <pre className="assistant-tool-call-detail-code">
              {result.result.path}
              {"\n"}
              {result.result.label}
              {result.result.isBinary ? "\n(бинарный файл)" : ""}
            </pre>
          </div>
          {!result.result.isBinary ? <DiffLines diff={result.result.diff} /> : null}
        </>
      );
    case "gitBlame":
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Git blame</div>
          <pre className="assistant-tool-call-detail-code">{result.result.path}</pre>
          {result.result.hunks.length === 0 ? (
            <p className="assistant-tool-call-detail-empty">Нет данных</p>
          ) : (
            <ul className="assistant-tool-call-detail-list">
              {result.result.hunks.map((hunk) => (
                <li key={`${hunk.commit}-${hunk.startLine}-${hunk.endLine}`}>
                  <span>
                    L{hunk.startLine}
                    {hunk.endLine !== hunk.startLine ? `–${hunk.endLine}` : ""} · {hunk.commit} ·{" "}
                    {hunk.author}
                    {hunk.summary ? ` · ${hunk.summary}` : ""}
                  </span>
                </li>
              ))}
            </ul>
          )}
          {result.result.truncated ? (
            <p className="assistant-tool-call-detail-empty">… blame обрезан</p>
          ) : null}
        </div>
      );
    case "todoWritten":
    case "todoUpdated":
      return <TodoChecklistDetail tasks={result.result} />;
    case "memory":
      return (
        <pre className="assistant-tool-call-detail-pre" style={{ whiteSpace: "pre-wrap" }}>
          {result.result.text}
        </pre>
      );
    default:
      return null;
  }
}

function todoGlyph(status: TodoStatus): string {
  switch (status) {
    case "completed":
      return "✓";
    case "inProgress":
      return "●";
    case "cancelled":
      return "✗";
    case "pending":
      return "○";
  }
}

/** Compact checklist rendering shared by `todoWritten`/`todoUpdated`'s
 * expanded detail view — both results carry the full, current `Task[]`, so
 * there's nothing op-specific to distinguish in the display itself. */
function TodoChecklistDetail({ tasks }: { tasks: Task[] }) {
  return (
    <div className="assistant-tool-call-detail-section">
      <div className="assistant-tool-call-detail-label">Список задач</div>
      {tasks.length === 0 ? (
        <p className="assistant-tool-call-detail-empty">Пусто</p>
      ) : (
        <ul className="assistant-tool-call-detail-list">
          {tasks.map((t) => (
            <li key={t.id}>
              <span>
                {todoGlyph(t.status)} {t.title}
                {t.note ? ` — ${t.note}` : ""}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
