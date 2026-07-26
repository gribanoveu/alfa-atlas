import { AlertCircle, AlertTriangle, Check, Loader2 } from "lucide-react";
import type { IndexStats } from "../../lib/workspaceIndex";
import type { IndexStatus, IndexProgress } from "../../hooks/useWorkspaceIndex";
import "./StatusBar.css";

type StatusBarProps = {
  filePath: string;
  formatLabel: string;
  lineEndingLabel: string;
  cursorLabel: string;
  hasActiveFile: boolean;
  indexStatus: IndexStatus;
  indexProgress: IndexProgress | null;
  indexStats: IndexStats | null;
};

function indexLabel(
  status: IndexStatus,
  progress: IndexProgress | null,
  stats: IndexStats | null,
): string {
  switch (status) {
    case "building":
      if (progress && progress.total > 0) {
        return `Building workspace index... (${progress.done} / ${progress.total})`;
      }
      if (progress && progress.current) {
        return `Updating index... (${progress.current})`;
      }
      return "Building workspace index...";
    case "ready":
      if (stats) {
        return `Indexed ${stats.documents} documents • ${stats.anchors} anchors • ${stats.references} xref • ${stats.includes} include • ${stats.warnings} warnings`;
      }
      return "Index ready";
    case "warning":
      if (stats && stats.errors > 0) {
        return `Index ready with ${stats.errors} error(s)`;
      }
      if (stats && stats.warnings > 0) {
        return `Index ready with ${stats.warnings} warning(s)`;
      }
      return "Index completed with warnings";
    case "error":
      return "Index failed";
    case "idle":
    default:
      return "—";
  }
}

function indexTitle(
  status: IndexStatus,
  stats: IndexStats | null,
): string {
  switch (status) {
    case "building":
      return "Building workspace index";
    case "ready":
      return stats
        ? `${stats.documents} docs, ${stats.anchors} anchors, ${stats.includes} includes, ${stats.references} xrefs, ${stats.images} images`
        : "Index ready";
    case "warning":
      if (stats && stats.errors > 0) {
        return `Index ready with ${stats.errors} error(s), ${stats.warnings} warning(s) — see Problems`;
      }
      return stats
        ? `Index ready with ${stats.warnings} warning(s)`
        : "Index ready with warnings";
    case "error":
      return "Index build failed — see Problems panel";
    default:
      return "Workspace index idle";
  }
}

export function StatusBar({
  filePath,
  formatLabel,
  lineEndingLabel,
  cursorLabel,
  hasActiveFile,
  indexStatus,
  indexProgress,
  indexStats,
}: StatusBarProps) {
  const showIndex = indexStatus !== "idle";
  const Icon =
    indexStatus === "building"
      ? Loader2
      : indexStatus === "ready"
        ? Check
        : indexStatus === "warning"
          ? AlertTriangle
          : indexStatus === "error"
            ? AlertCircle
            : null;

  return (
    <footer className="statusbar">
      <div className="seg" title={filePath}>
        {filePath}
      </div>
      <div className="grow" />
      {showIndex ? (
        <div
          className={`seg ai ${indexStatus}`}
          title={indexTitle(indexStatus, indexStats)}
        >
          {Icon ? (
            <Icon
              size={11}
              className={indexStatus === "building" ? "spin" : ""}
            />
          ) : null}
          {indexLabel(indexStatus, indexProgress, indexStats)}
        </div>
      ) : null}
      {hasActiveFile ? (
        <>
          <div className="seg" title="Формат файла">
            {formatLabel}
          </div>
          <div className="seg" title="Окончания строк">
            {lineEndingLabel}
          </div>
          <div className="seg" title="Позиция курсора">
            {cursorLabel}
          </div>
        </>
      ) : (
        <>
          <div className="seg">—</div>
          <div className="seg">{cursorLabel}</div>
        </>
      )}
    </footer>
  );
}
