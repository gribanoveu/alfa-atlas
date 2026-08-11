import { syncPillLabel, type SyncPillState } from "../../lib/git";

type SyncStatusPillProps = {
  state: SyncPillState;
  onClick: () => void;
};

export function SyncStatusPill({ state, onClick }: SyncStatusPillProps) {
  const label = syncPillLabel(state);
  return (
    <button
      type="button"
      className={`sync-pill sync-pill-${state}`}
      onClick={onClick}
      title={label}
    >
      {label}
    </button>
  );
}
