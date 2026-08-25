import {
  AlertCircle,
  ArrowDownToLine,
  ArrowUpFromLine,
  Check,
  CircleDot,
  GitMerge,
} from "lucide-react";
import { syncPillLabel, syncPillShortLabel, type SyncPillState } from "../../lib/git";

type SyncStatusPillProps = {
  state: SyncPillState;
  onClick: () => void;
};

function SyncPillIcon({ state }: { state: SyncPillState }) {
  const size = 12;
  switch (state) {
    case "conflict":
      return <AlertCircle size={size} aria-hidden />;
    case "merging":
      return <GitMerge size={size} aria-hidden />;
    case "dirty":
      return <CircleDot size={size} aria-hidden />;
    case "behind":
      return <ArrowDownToLine size={size} aria-hidden />;
    case "unpushed":
      return <ArrowUpFromLine size={size} aria-hidden />;
    case "synced":
      return <Check size={size} aria-hidden />;
  }
}

export function SyncStatusPill({ state, onClick }: SyncStatusPillProps) {
  const title = syncPillLabel(state);
  const label = syncPillShortLabel(state);
  return (
    <button
      type="button"
      className={`sync-pill sync-pill-${state}`}
      onClick={onClick}
      title={title}
      aria-label={title}
    >
      <SyncPillIcon state={state} />
      <span className="sync-pill-label">{label}</span>
    </button>
  );
}
