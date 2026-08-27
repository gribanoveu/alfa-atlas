import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { memoryRowKey, useMemoryLog } from "../../hooks/useMemoryLog";
import type { MemoryLogRow } from "../../lib/memoryLog";
import "../Welcome/CloneRepoModal.css";
import "../ToolLog/ToolCallLogModal.css";
import "./MemoryLogModal.css";

type LogSelectOption = { value: string; label: string };

function LogSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: LogSelectOption[];
  onChange: (value: string) => void;
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

  const current = options.find((o) => o.value === value) ?? options[0];

  return (
    <div className="clone-select tool-log-select" ref={ref}>
      <button
        type="button"
        className={`clone-select-trigger${open ? " is-open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={label}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="clone-select-value">
          <span className="clone-select-path">{current?.label}</span>
        </span>
        <span className="clone-select-chevron" aria-hidden>
          ▾
        </span>
      </button>
      {open ? (
        <div className="clone-select-menu" role="listbox">
          {options.map((option) => {
            const active = option.value === value;
            return (
              <button
                key={option.value}
                type="button"
                role="option"
                aria-selected={active}
                className={`clone-select-option${active ? " is-active" : ""}`}
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                }}
              >
                <span className="clone-select-path">{option.label}</span>
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

type MemoryLogModalProps = {
  projectRoot: string | null;
  onClose: () => void;
};


const SCOPE_OPTIONS: LogSelectOption[] = [
  { value: "", label: "Все области" },
  { value: "project", label: "Проектная" },
  { value: "global", label: "Глобальная" },
];

function scopeLabel(scope: MemoryLogRow["scope"]): string {
  return scope === "global" ? "глобальная" : scope === "project" ? "проектная" : scope;
}

function previewText(text: string, max = 120): string {
  const oneLine = text.replace(/\s+/g, " ").trim();
  if (oneLine.length <= max) return oneLine;
  return `${oneLine.slice(0, max)}…`;
}

export function MemoryLogModal({ projectRoot, onClose }: MemoryLogModalProps) {
  const {
    rows,
    projectStorePath,
    globalStorePath,
    loading,
    error,
    deletingKey,
    search,
    setSearch,
    scope,
    setScope,
    canPrev,
    canNext,
    rangeLabel,
    prevPage,
    nextPage,
    deleteEntry,
  } = useMemoryLog(projectRoot);

  // Which row is unfolded — presentation only.
  const [expandedKey, setExpandedKey] = useState<string | null>(null);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const handleDelete = useCallback(
    async (row: MemoryLogRow) => {
      const label = previewText(row.text, 80);
      const ok = window.confirm(
        `Удалить запись #${row.id} (${scopeLabel(row.scope)})?\n\n${label}`,
      );
      if (!ok) return;
      if (await deleteEntry(row)) {
        if (expandedKey === memoryRowKey(row)) setExpandedKey(null);
      }
    },
    [deleteEntry, expandedKey],
  );


  return (
    <div className="tool-log-backdrop" role="presentation" onClick={onClose}>
      <div
        className="tool-log-dialog"
        role="dialog"
        aria-labelledby="memory-log-dialog-title"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="tool-log-header">
          <div>
            <h2 className="tool-log-title" id="memory-log-dialog-title">
              Память ассистента
            </h2>
            <p className="memory-log-subtitle">
              {projectStorePath ? (
                <>
                  Проект: <span title={projectStorePath}>{projectStorePath}</span>
                  {" · "}
                </>
              ) : null}
              Глобальная: <span title={globalStorePath}>{globalStorePath || "—"}</span>
            </p>
          </div>
          <div className="tool-log-header-actions">
            <button type="button" className="tool-log-close" onClick={onClose} aria-label="Закрыть">
              ×
            </button>
          </div>
        </header>

        <div className="tool-log-filters">
          <input
            type="text"
            className="tool-log-search"
            placeholder="Поиск по тексту записи…"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
          />
          <LogSelect label="Область" value={scope} options={SCOPE_OPTIONS} onChange={setScope} />
          {!projectRoot ? (
            <span className="memory-log-hint">Откройте проект, чтобы видеть проектную память</span>
          ) : null}
        </div>

        {error ? <div className="tool-log-error">{error}</div> : null}

        <div className="tool-log-body">
          <table className="tool-log-table">
            <thead>
              <tr>
                <th>#</th>
                <th>Дата</th>
                <th>Область</th>
                <th>Запись</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const rowKey = `${row.scope}-${row.id}`;
                return (
                  <Fragment key={rowKey}>
                    <tr
                      className={`tool-log-row${expandedKey === rowKey ? " is-expanded" : ""}`}
                      onClick={() => setExpandedKey(expandedKey === rowKey ? null : rowKey)}
                    >
                      <td>{row.id}</td>
                      <td>{row.date}</td>
                      <td>
                        <span className={`memory-log-scope memory-log-scope-${row.scope}`}>
                          {scopeLabel(row.scope)}
                        </span>
                      </td>
                      <td className="memory-log-text">{previewText(row.text)}</td>
                    </tr>
                    {expandedKey === rowKey ? (
                      <tr className="tool-log-detail-row">
                        <td colSpan={4}>
                          <div className="tool-log-detail">
                            <div className="tool-log-detail-meta" title={row.storePath}>
                              {row.storePath}
                            </div>
                            <pre className="tool-log-detail-json">{row.text}</pre>
                            <div className="memory-log-detail-actions">
                              <button
                                type="button"
                                className="tool-log-btn memory-log-delete-btn"
                                disabled={deletingKey === rowKey}
                                onClick={(event) => {
                                  event.stopPropagation();
                                  void handleDelete(row);
                                }}
                              >
                                {deletingKey === rowKey ? "Удаление…" : "Удалить запись"}
                              </button>
                            </div>
                          </div>
                        </td>
                      </tr>
                    ) : null}
                  </Fragment>
                );
              })}
              {!loading && rows.length === 0 ? (
                <tr>
                  <td colSpan={4} className="tool-log-empty">
                    {projectRoot || scope === "global"
                      ? "Память пуста или ничего не найдено"
                      : "Откройте проект или выберите «Глобальная»"}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>

        <footer className="tool-log-footer">
          <span className="tool-log-range">{loading ? "Загрузка…" : rangeLabel}</span>
          <div className="tool-log-pagination">
            <button
              type="button"
              className="tool-log-btn"
              disabled={!canPrev || loading}
              onClick={prevPage}
            >
              Назад
            </button>
            <button
              type="button"
              className="tool-log-btn"
              disabled={!canNext || loading}
              onClick={nextPage}
            >
              Вперёд
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}
