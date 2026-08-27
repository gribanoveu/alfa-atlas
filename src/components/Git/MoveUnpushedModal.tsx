import { useEffect, useRef, useState } from "react";
import type { GitBranchInfo, GitCommitSummary } from "../../lib/git";
import { GitCommitList } from "./GitCommitList";
import "../Welcome/CloneRepoModal.css";
import "./MoveUnpushedModal.css";

const DEFAULT_BRANCH_PREFIX = "doc/";

type MoveTarget = "new" | "existing";

type MoveUnpushedModalProps = {
  currentBranch: string | null;
  branches: GitBranchInfo[];
  commits: GitCommitSummary[];
  busy: boolean;
  onCancel: () => void;
  onConfirm: (target: { kind: "new"; name: string } | { kind: "existing"; branch: string }) => void;
};

function BranchSelect({
  value,
  branches,
  disabled,
  onChange,
}: {
  value: string;
  branches: GitBranchInfo[];
  disabled: boolean;
  onChange: (branch: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const selected = branches.find((branch) => branch.name === value);

  return (
    <div className="clone-select move-unpushed-select" ref={ref}>
      <button
        type="button"
        className={`clone-select-trigger${open ? " is-open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label="Выберите ветку"
        disabled={disabled || branches.length === 0}
        onClick={() => setOpen((current) => !current)}
      >
        <span className="clone-select-value">
          {selected ? (
            <span className="clone-select-path">{selected.name}</span>
          ) : (
            <span className="clone-select-placeholder">
              {branches.length === 0 ? "Нет других локальных веток" : "Выберите ветку…"}
            </span>
          )}
        </span>
        <span className="clone-select-chevron" aria-hidden>
          ▾
        </span>
      </button>
      {open ? (
        <div className="clone-select-menu" role="listbox">
          {branches.length === 0 ? (
            <div className="clone-select-option">
              <span className="clone-select-path">Нет других локальных веток</span>
            </div>
          ) : (
            branches.map((branch) => {
              const active = branch.name === value;
              return (
                <button
                  key={branch.name}
                  type="button"
                  role="option"
                  aria-selected={active}
                  className={`clone-select-option${active ? " is-active" : ""}`}
                  onClick={() => {
                    onChange(branch.name);
                    setOpen(false);
                  }}
                >
                  <span className="clone-select-path">{branch.name}</span>
                </button>
              );
            })
          )}
        </div>
      ) : null}
    </div>
  );
}

export function MoveUnpushedModal({
  currentBranch,
  branches,
  commits,
  busy,
  onCancel,
  onConfirm,
}: MoveUnpushedModalProps) {
  const [target, setTarget] = useState<MoveTarget>("new");
  const [newBranchName, setNewBranchName] = useState(DEFAULT_BRANCH_PREFIX);
  const [existingBranch, setExistingBranch] = useState("");

  const localBranches = branches.filter((b) => !b.isRemote && !b.isCurrent);

  const canConfirm =
    target === "new"
      ? newBranchName.trim().length > 0
      : existingBranch.length > 0;

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (!busy && event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        className="clone-modal move-unpushed-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="move-unpushed-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="move-unpushed-title">
          Перенести неотправленные коммиты
        </div>
        <div className="clone-modal-message">
          {currentBranch
            ? `Коммиты будут перенесены с «${currentBranch}» и удалены с текущей ветки (если у неё есть upstream).`
            : "Коммиты будут перенесены на выбранную ветку."}
        </div>

        <GitCommitList commits={commits} emptyMessage="Нет неотправленных коммитов" />

        <fieldset className="move-unpushed-options" disabled={busy}>
          <label className="move-unpushed-option">
            <input
              type="radio"
              name="move-target"
              checked={target === "new"}
              onChange={() => setTarget("new")}
            />
            <span>Новая ветка</span>
          </label>
          {target === "new" ? (
            <input
              type="text"
              className="move-unpushed-input"
              placeholder="doc/feature-name"
              value={newBranchName}
              onChange={(event) => setNewBranchName(event.target.value)}
              disabled={busy}
              autoFocus
            />
          ) : null}

          <label className="move-unpushed-option">
            <input
              type="radio"
              name="move-target"
              checked={target === "existing"}
              onChange={() => setTarget("existing")}
            />
            <span>Существующая ветка</span>
          </label>
          {target === "existing" ? (
            <BranchSelect
              value={existingBranch}
              branches={localBranches}
              disabled={busy}
              onChange={setExistingBranch}
            />
          ) : null}
        </fieldset>

        <div className="clone-modal-actions">
          <button type="button" className="clone-modal-btn" disabled={busy} onClick={onCancel}>
            Отмена
          </button>
          <button
            type="button"
            className="clone-modal-btn primary"
            disabled={busy || !canConfirm || commits.length === 0}
            onClick={() => {
              if (target === "new") {
                onConfirm({ kind: "new", name: newBranchName.trim() });
              } else {
                onConfirm({ kind: "existing", branch: existingBranch });
              }
            }}
          >
            {busy ? "Перенос…" : "Перенести"}
          </button>
        </div>
      </div>
    </div>
  );
}
