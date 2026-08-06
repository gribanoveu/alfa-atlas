import { AlertCircle, AlertTriangle, Check, Loader2 } from "lucide-react";
import type { IndexStats } from "../../lib/workspaceIndex";
import type { IndexStatus, IndexProgress } from "../../hooks/useWorkspaceIndex";
import type { EmbeddingIndexStatus, SyncProgress } from "../../lib/embeddings";
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
  /** `null` before the first fetch resolves, or when no project is open —
   * the embedding/RAG index segment hides entirely in that case, same as
   * the workspace index segment hides for `indexStatus === "idle"`. */
  embedIndexStatus: EmbeddingIndexStatus | null;
  embedSyncProgress: SyncProgress | null;
};

type EmbedIndexState = "syncing" | "stale" | "synced" | "unsynced";

function embedIndexState(
  status: EmbeddingIndexStatus | null,
  progress: SyncProgress | null,
): EmbedIndexState | null {
  if (progress) return "syncing";
  if (!status) return null;
  if (status.stale) return "stale";
  if (status.synced) return "synced";
  return "unsynced";
}

function embedIndexLabel(
  state: EmbedIndexState,
  status: EmbeddingIndexStatus | null,
  progress: SyncProgress | null,
): string {
  switch (state) {
    case "syncing": {
      const phase = progress?.phase === "chunking" ? "Индексация файлов" : "Расчёт эмбеддингов";
      return progress && progress.total > 0
        ? `${phase}: ${progress.current}/${progress.total}`
        : `${phase}…`;
    }
    case "stale":
      return "Индекс устарел";
    case "synced": {
      const base = `Проиндексировано чанков: ${status?.embeddedCount ?? 0}`;
      return status && status.backgroundPending > 0
        ? `${base} (+${status.backgroundPending} в фоне)`
        : base;
    }
    case "unsynced":
      return "Индекс не синхронизирован";
  }
}

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
  embedIndexStatus,
  embedSyncProgress,
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

  const embedState = embedIndexState(embedIndexStatus, embedSyncProgress);
  const EmbedIcon =
    embedState === "syncing"
      ? Loader2
      : embedState === "synced"
        ? Check
        : embedState === "stale"
          ? AlertTriangle
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
      {embedState ? (
        <div className={`seg embed ${embedState}`} title="Индекс эмбеддингов (документация и репозиторий)">
          {EmbedIcon ? (
            <EmbedIcon size={11} className={embedState === "syncing" ? "spin" : ""} />
          ) : null}
          {embedIndexLabel(embedState, embedIndexStatus, embedSyncProgress)}
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
