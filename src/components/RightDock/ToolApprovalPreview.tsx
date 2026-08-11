import { File } from "lucide-react";
import { useEffect, useState } from "react";
import { basename, TOOL_APPROVAL_TIMEOUT_MS } from "../../lib/assistantConfig";
import type { FileEdit } from "../../lib/aiTools";
import type { ToolCallBlock } from "../../lib/chatBlocks";
import { DeleteDirectoryReview } from "./DeleteDirectoryReview";
import { DeleteFileReview } from "./DeleteFileReview";
import { EditFileDiffReview } from "./EditFileDiffReview";
import { WriteFileDiffReview } from "./WriteFileDiffReview";

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

/** Whether this call's preview has a collapsible body — a diff or content
 * view the item row's own title toggles open/closed — as opposed to
 * always-visible static info (`move`'s note, `requestFullRepoAccess`'s
 * reason, …) or nothing at all (a plain `createDirectory`). Mirrors the
 * `block.name`/`args` guards in `ToolApprovalPreview` below; kept in sync
 * with it by hand since both read the same `args` shape. `AssistantToolApprovalGroup`
 * uses this to decide whether an item's title renders as a clickable
 * expand/collapse control or as plain static text. */
export function isExpandableToolCall(block: ToolCallBlock): boolean {
  const args = parseArgs(block.argumentsJson);
  switch (block.name) {
    case "writeFile":
      return typeof args.path === "string" && typeof args.content === "string";
    case "editFile":
      return typeof args.path === "string" && isFileEditArray(args.edits);
    case "deleteFile":
    case "deleteDirectory":
      return typeof args.path === "string";
    default:
      return false;
  }
}

/** "Правок: N" badge for a pending `editFile` call, shown next to its item
 * title — `null` for every other tool name or malformed `edits`. */
export function editCountBadge(block: ToolCallBlock): string | null {
  if (block.name !== "editFile") return null;
  const args = parseArgs(block.argumentsJson);
  return isFileEditArray(args.edits) ? `Правок: ${args.edits.length}` : null;
}

/** Tool-specific preview body for one pending call — extracted from the old
 * single-call `ToolApprovalCard` so both `AssistantToolApprovalGroup` (one
 * row per call) and any future single-call use share the exact same
 * rendering. Never repeats the path/filename here: the row this sits under
 * (`AssistantToolApprovalGroup`'s item title, via `describeToolActivity`)
 * already states it. A diff/content body (`isExpandableToolCall`) is
 * controlled entirely by the caller via `expanded` — that item's own title
 * is the toggle, not anything rendered in here — so it's simply omitted
 * while collapsed; static info (the `move` note, `requestFullRepoAccess`'s
 * reason, …) always renders regardless of `expanded`. */
export function ToolApprovalPreview({
  block,
  docsRoot,
  expanded,
}: {
  block: ToolCallBlock;
  docsRoot: string;
  expanded: boolean;
}) {
  const args = parseArgs(block.argumentsJson);

  return block.name === "writeFile" && typeof args.path === "string" && typeof args.content === "string" ? (
    expanded ? (
      <div className="assistant-tool-call-detail-section">
        <WriteFileDiffReview docsRoot={docsRoot} path={args.path} content={args.content} />
      </div>
    ) : null
  ) : block.name === "editFile" && typeof args.path === "string" && isFileEditArray(args.edits) ? (
    expanded ? (
      <div className="assistant-tool-call-detail-section">
        <EditFileDiffReview docsRoot={docsRoot} path={args.path} edits={args.edits} />
      </div>
    ) : null
  ) : block.name === "createDirectory" && typeof args.path === "string" && args.template === "restEndpoint" ? (
    <div className="assistant-tool-call-detail-section">
      <div className="assistant-tool-call-detail-label">Будут созданы файлы</div>
      <ul className="assistant-tool-call-detail-list">
        {restEndpointPreviewFiles(args.path).map((file) => (
          <li key={file} title={file}>
            <File className="assistant-tool-call-detail-icon" size={12} aria-hidden />
            <span>{basename(file)}</span>
          </li>
        ))}
      </ul>
    </div>
  ) : block.name === "deleteFile" && typeof args.path === "string" ? (
    expanded ? (
      <div className="assistant-tool-call-detail-section">
        <DeleteFileReview docsRoot={docsRoot} path={args.path} />
      </div>
    ) : null
  ) : block.name === "deleteDirectory" && typeof args.path === "string" ? (
    expanded ? (
      <div className="assistant-tool-call-detail-section">
        <DeleteDirectoryReview docsRoot={docsRoot} path={args.path} />
      </div>
    ) : null
  ) : block.name === "move" && typeof args.path === "string" && typeof args.newPath === "string" ? (
    <div className="assistant-tool-call-detail-section">
      <div className="assistant-tool-approval-reason">
        Ссылки на файл в других документах будут обновлены автоматически.
      </div>
    </div>
  ) : block.name === "requestFullRepoAccess" && typeof args.reason === "string" ? (
    <div className="assistant-tool-call-detail-section">
      <div className="assistant-tool-call-detail-label">Причина</div>
      <div className="assistant-tool-approval-reason">{args.reason}</div>
    </div>
  ) : block.name === "requestModeSwitch" && typeof args.reason === "string" ? (
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
  ) : null;
}

/** A strip that visually depletes from full to empty over the time
 * remaining until `deadlineAt` (`useLlmChat`'s `TOOL_APPROVAL_TIMEOUT_MS`
 * auto-deny deadline) — a single CSS `width` transition kicked off one
 * frame after mount, not a per-frame JS timer, so it stays smooth
 * regardless of render cadence. Purely visual: the actual auto-deny is a
 * real `setTimeout` in `useLlmChat`, independent of whether this component
 * is even mounted to show it. */
export function ApprovalCountdown({ deadlineAt }: { deadlineAt?: number }) {
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
