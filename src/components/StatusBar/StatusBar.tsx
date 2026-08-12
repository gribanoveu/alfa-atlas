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
  /** Clicking the embeddings segment triggers the same sync
   * `EmbeddingsTab.tsx`'s own button calls. `undefined` (no project open)
   * renders the segment as a plain, non-interactive `<div>` again. */
  onEmbedSyncClick?: () => void;
  /** While `true`, the segment still shows (and reacts to hover/click
   * cosmetically), but the click itself is a no-op — mirrors
   * `EmbeddingsTab.tsx`'s own `busy || syncing || !providerConfigured`
   * guard on its sync button. */
  embedSyncDisabled?: boolean;
  /** Shows a brief "Синхронизировано" confirmation in place of the normal
   * label right after a successful click-to-sync — see `App.tsx`'s
   * `embedJustSynced` state for the timing. */
  embedJustSynced?: boolean;
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
        return `Индексация файлов: ${progress.done} / ${progress.total}`;
      }
      if (progress && progress.current) {
        return `Обновление файлов: ${progress.current}`;
      }
      return "Индексация файлов…";
    case "ready":
      return stats ? `Проверено файлов: ${stats.documents}` : "Индекс готов";
    case "warning":
      if (stats && stats.errors > 0) {
        return `Ошибок в файлах: ${stats.errors}`;
      }
      if (stats && stats.warnings > 0) {
        return `Предупреждений в файлах: ${stats.warnings}`;
      }
      return "Файлы проверены с предупреждениями";
    case "error":
      return "Ошибка проверки файлов";
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
      return "Проверка файлов";
    case "ready":
      return stats
        ? `Документов: ${stats.documents} • якорей: ${stats.anchors} • включений: ${stats.includes} • перекрёстных ссылок: ${stats.references} • изображений: ${stats.images}`
        : "Файлы проверены";
    case "warning":
      if (stats && stats.errors > 0) {
        return `Ошибок в файлах: ${stats.errors}, предупреждений: ${stats.warnings} — см. панель Проблемы`;
      }
      return stats
        ? `Предупреждений в файлах: ${stats.warnings}`
        : "Файлы проверены с предупреждениями";
    case "error":
      return "Ошибка проверки файлов — см. панель Проблемы";
    default:
      return "Файлы не проверены";
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
  onEmbedSyncClick,
  embedSyncDisabled,
  embedJustSynced,
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
        onEmbedSyncClick ? (
          <button
            type="button"
            className={`seg embed clickable ${embedState}${embedJustSynced ? " just-synced" : ""}`}
            title={
              embedSyncDisabled
                ? "Индекс эмбеддингов (документация и репозиторий)"
                : "Индекс эмбеддингов (документация и репозиторий) — нажмите, чтобы синхронизировать"
            }
            disabled={embedSyncDisabled}
            onClick={onEmbedSyncClick}
          >
            {embedJustSynced ? (
              <Check size={11} />
            ) : EmbedIcon ? (
              <EmbedIcon size={11} className={embedState === "syncing" ? "spin" : ""} />
            ) : null}
            {embedJustSynced ? "Синхронизировано" : embedIndexLabel(embedState, embedIndexStatus, embedSyncProgress)}
          </button>
        ) : (
          <div className={`seg embed ${embedState}`} title="Индекс эмбеддингов (документация и репозиторий)">
            {EmbedIcon ? (
              <EmbedIcon size={11} className={embedState === "syncing" ? "spin" : ""} />
            ) : null}
            {embedIndexLabel(embedState, embedIndexStatus, embedSyncProgress)}
          </div>
        )
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
