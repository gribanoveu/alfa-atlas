import { AlertCircle, Bot, Check, ChevronDown, ChevronRight, Clock, File, Folder, Loader2 } from "lucide-react";
import { useState } from "react";
import { describeMatchSource, describeToolActivity, describeToolResult, formatToolArguments } from "../../lib/assistantConfig";
import type { FileDiffStats, Task, ToolResult, TodoStatus } from "../../lib/aiTools";
import { normalizeSemanticSearchResult } from "../../lib/aiTools";
import type { ToolCallBlock } from "../../lib/chatBlocks";

/** A file's content can be arbitrarily large — this caps how much of it the
 * expanded detail view renders, purely as a rendering safeguard (the model
 * itself still received the untruncated content; this doesn't change what
 * was sent). */
const MAX_DETAIL_CHARS = 4000;
const FINDING_PREVIEW_CHARS = 120;

function truncateForDisplay(text: string): { text: string; truncated: boolean } {
  if (text.length <= MAX_DETAIL_CHARS) return { text, truncated: false };
  return { text: text.slice(0, MAX_DETAIL_CHARS), truncated: true };
}

function findingPreview(message: string): string {
  const firstLine = (message.split("\n")[0] ?? message).trim();
  if (firstLine.length <= FINDING_PREVIEW_CHARS) return firstLine;
  return `${firstLine.slice(0, FINDING_PREVIEW_CHARS - 1)}…`;
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
};

/** One permanent, chronological entry for a single tool invocation inside
 * an assistant message's transcript — a status icon, the "what is/was being
 * done" line (`describeToolActivity`, reused as-is regardless of status),
 * and once settled, a dimmed one-line result summary (`describeToolResult`).
 * Never disappears once appended, unlike the old transient `toolActivity`
 * list it replaces — see `useLlmChat`'s `MessageBlock` model.
 *
 * A `"pendingApproval"` block never reaches this component —
 * `groupBlocksForRender` (`src/lib/chatBlocks.ts`) diverts every call still
 * awaiting a decision into `AssistantToolApprovalGroup` instead; this
 * component only ever renders `"running"`/`"done"`/`"error"`.
 *
 * Clicking the header expands a detail view — the raw arguments plus,
 * once settled, the full result (file content / file list / search
 * matches) or the full error — for inspecting exactly what happened,
 * beyond the one-line summary. Collapsed by default, matching Cursor/
 * Claude Code's own collapsed-by-default tool-call display. */
export function AssistantToolCallBlock({ block }: AssistantToolCallBlockProps) {
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
          <Loader2 className="assistant-tool-call-icon assistant-chat-tool-spinner" size={15} aria-hidden />
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
    case "semanticSearchResults": {
      const { matches, meta } = normalizeSemanticSearchResult(result.result);
      // `meta.degraded` is worded for the model (it replaces `meta.hint` on
      // the wire, see `tools::semantic_search::degraded_note`), so the user
      // gets their own phrasing here — and the ordinary hint is suppressed,
      // since the degradation is both the cause and the only useful advice.
      const degradedText = meta.degraded
        ? "Семантический поиск был недоступен — показаны совпадения только по именам и тексту. Проверьте доступ к провайдеру эмбеддингов."
        : null;
      const hintText =
        degradedText ??
        meta.hint ??
        (meta.weak
          ? "Поиск дал слабые результаты — уточните запрос английскими именами методов/классов."
          : null);
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Результаты</div>
          {matches.length === 0 ? (
            <p className="assistant-tool-call-detail-empty">Ничего не найдено</p>
          ) : (
            <ul className="assistant-tool-call-detail-list assistant-tool-call-detail-matches">
              {matches.map((match, i) => {
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
                    {match.qualifiedName ? (
                      <div className="assistant-tool-call-detail-match-qualified">
                        {match.qualifiedName}
                      </div>
                    ) : null}
                    <div className="assistant-tool-call-detail-match-snippet">{match.snippet}</div>
                  </li>
                );
              })}
            </ul>
          )}
          {hintText ? (
            <p className="assistant-tool-call-detail-hint">
              <AlertCircle size={14} strokeWidth={1.75} aria-hidden />
              <span>{hintText}</span>
            </p>
          ) : null}
        </div>
      );
    }
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
                  <li key={f.folder} className="assistant-tool-call-standards-folder">
                    <span>
                      [{f.passed ? "✓" : "✗"} {pct}%] {f.folder}
                    </span>
                    {failing.length > 0 ? (
                      <ul className="assistant-tool-call-standards-fails">
                        {failing.map((finding) => (
                          <li key={finding.ruleId}>
                            {finding.ruleId} — {findingPreview(finding.message)}
                          </li>
                        ))}
                      </ul>
                    ) : null}
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
    case "asciidocTemplates":
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Шаблоны AsciiDoc</div>
          {result.result.templates.length === 0 ? (
            <p className="assistant-tool-call-detail-empty">Ничего не найдено</p>
          ) : (
            result.result.templates.map((t) => (
              <div key={t.id}>
                <div className="assistant-tool-call-detail-label">{t.label}</div>
                <pre className="assistant-tool-call-detail-code">{t.template}</pre>
              </div>
            ))
          )}
          {result.result.notFound.length > 0 ? (
            <p className="assistant-tool-call-detail-empty">
              Не найдено: {result.result.notFound.join(", ")}
            </p>
          ) : null}
        </div>
      );
    case "skillSearch":
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">Скилы</div>
          {result.result.matches.length === 0 ? (
            <p className="assistant-tool-call-detail-empty">Ничего не найдено</p>
          ) : (
            <ul className="assistant-tool-call-detail-list">
              {result.result.matches.map((hit) => (
                <li key={`${hit.source}:${hit.name}`}>
                  <span>
                    {hit.name}
                    {hit.source === "user" ? " · пользовательский" : ""} — {hit.description}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      );
    case "skillLoaded":
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">{result.result.name}</div>
          <pre className="assistant-tool-call-detail-pre" style={{ whiteSpace: "pre-wrap" }}>
            {result.result.body}
          </pre>
          {result.result.files.length > 0 ? (
            <p className="assistant-tool-call-detail-empty">
              Файлы: {result.result.files.join(", ")}
            </p>
          ) : null}
        </div>
      );
    case "skillFile":
      return (
        <div className="assistant-tool-call-detail-section">
          <div className="assistant-tool-call-detail-label">{result.result.path}</div>
          <pre className="assistant-tool-call-detail-pre" style={{ whiteSpace: "pre-wrap" }}>
            {result.result.content}
          </pre>
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
