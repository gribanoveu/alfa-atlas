import { GitFork, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import type { GitBranchInfo } from "../../lib/git";
import "./BranchesPanel.css";

const DEFAULT_BRANCH_PREFIX = "doc/";

export type BranchesPanelProps = {
  currentBranch: string;
  branches: GitBranchInfo[];
  busy: boolean;
  error: string | null;
  onCheckout: (name: string) => void;
  onCreateBranch: (name: string) => void;
  onRefresh: () => void;
};

function splitBranchName(name: string): { leaf: string; prefix: string } {
  const idx = name.lastIndexOf("/");
  if (idx < 0) return { leaf: name, prefix: "" };
  return {
    leaf: name.slice(idx + 1),
    prefix: name.slice(0, idx + 1),
  };
}

export function BranchesPanel({
  currentBranch,
  branches,
  busy,
  error,
  onCheckout,
  onCreateBranch,
  onRefresh,
}: BranchesPanelProps) {
  const [newBranchName, setNewBranchName] = useState(DEFAULT_BRANCH_PREFIX);
  const [search, setSearch] = useState("");

  const sortedBranches = useMemo(() => {
    const current = branches.find((branch) => branch.isCurrent);
    const others = branches
      .filter((branch) => !branch.isCurrent)
      .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
    return current ? [current, ...others] : others;
  }, [branches]);

  const filteredBranches = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return sortedBranches;
    return sortedBranches.filter((branch) =>
      branch.name.toLowerCase().includes(query),
    );
  }, [search, sortedBranches]);

  const canCreate = newBranchName.trim().length > 0 && !busy;

  return (
    <div className="branches-panel" id="branches-panel">
      <div className="branches-panel-toolbar">
        <span className="branches-panel-toolbar-title">
          Локальные ветки
          <span className="branches-panel-toolbar-count">({branches.length})</span>
        </span>
        <button
          type="button"
          className="branches-icon-btn"
          title="Обновить список"
          aria-label="Обновить список веток"
          disabled={busy}
          onClick={onRefresh}
        >
          <RefreshCw size={14} aria-hidden />
        </button>
      </div>

      <div className="branches-panel-search-wrap">
        <input
          type="search"
          className="branches-panel-search"
          placeholder="Поиск ветки…"
          value={search}
          disabled={busy}
          spellCheck={false}
          onChange={(event) => setSearch(event.target.value)}
        />
      </div>

      <div className="branches-panel-scroll">
        {filteredBranches.length === 0 ? (
          <div className="branches-panel-empty">
            {branches.length === 0
              ? "Нет локальных веток"
              : "Ничего не найдено по запросу"}
          </div>
        ) : (
          <ul className="branches-panel-list" role="list">
            {filteredBranches.map((branch) => {
              const { leaf, prefix } = splitBranchName(branch.name);
              return (
                <li key={branch.name}>
                  <button
                    type="button"
                    className={`branches-panel-item${branch.isCurrent ? " is-current" : ""}`}
                    disabled={busy || branch.isCurrent}
                    title={branch.name}
                    onClick={() => onCheckout(branch.name)}
                  >
                    <GitFork
                      className="branches-panel-item-icon"
                      size={13}
                      aria-hidden
                    />
                    <span className="branches-panel-item-text">
                      <span className="branches-panel-item-leaf">{leaf}</span>
                      {prefix ? (
                        <span className="branches-panel-item-prefix">{prefix}</span>
                      ) : null}
                    </span>
                    {branch.isCurrent ? (
                      <span className="branches-panel-badge">текущая</span>
                    ) : null}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      <form
        className="branches-panel-create-dock"
        onSubmit={(event) => {
          event.preventDefault();
          const name = newBranchName.trim();
          if (!name || busy) return;
          onCreateBranch(name);
          setNewBranchName(DEFAULT_BRANCH_PREFIX);
        }}
      >
        <div className="branches-panel-create-row">
          <input
            type="text"
            className="branches-panel-input"
            placeholder="doc/feature-name"
            value={newBranchName}
            disabled={busy}
            spellCheck={false}
            onChange={(event) => setNewBranchName(event.target.value)}
          />
          <button
            type="submit"
            className="branches-panel-create-btn"
            disabled={!canCreate}
            title={
              canCreate
                ? "Создать ветку и переключиться"
                : "Введите имя новой ветки"
            }
          >
            Создать ветку
          </button>
        </div>
        <p className="branches-panel-hint">
          От текущей ветки <strong>{currentBranch}</strong> будет создана новая ветка с именем <strong>{newBranchName}</strong>. 
        </p>
        <p className="branches-panel-hint">
            Если у вас есть файлы добавленные в коммит, но коммит не сделан, то удалите их из отслеживания или сделайте коммит,
            иначе переключение будет заблокировано.
        </p>
        {error ? <div className="branches-panel-error">{error}</div> : null}
      </form>
    </div>
  );
}
