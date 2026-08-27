import { Fragment, useEffect, useRef, useState } from "react";
import {
  DATE_RANGE_OPTIONS,
  useToolCallLog,
  type DateRangeOption,
} from "../../hooks/useToolCallLog";
import "../Welcome/CloneRepoModal.css";
import "./ToolCallLogModal.css";

type LogSelectOption = { value: string; label: string };

// Same `.clone-select*` trigger/menu markup every other dropdown in the app
// hand-rolls per usage (`SettingsDialog`'s language picker,
// `AssistantConversation`'s model picker, …) — factored into one local
// component here only because this file needs three instances of it side by
// side, not as a new sitewide abstraction.
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

type ToolCallLogModalProps = {
  projectRoot: string | null;
  onClose: () => void;
};


// Every `ToolName` variant (`domain::ai_access::ToolName`), in the same
// camelCase spelling the wire protocol/log rows already use — a fixed list
// rather than deriving it from loaded rows so the filter still offers every
// tool even when the log is empty or only has a few kinds so far.
const KNOWN_TOOLS = [
  "listFiles",
  "readFile",
  "semanticSearch",
  "grep",
  "gitDiff",
  "gitBlame",
  "check",
  "writeFile",
  "editFile",
  "deleteFile",
  "createDirectory",
  "deleteDirectory",
  "move",
  "requestFullRepoAccess",
  "todo",
  "memory",
  "requestModeSwitch",
  "getAsciidocTemplates",
  "askUser",
  "createPlan",
  "updatePlan",
  "readPlan",
  "updatePlanTodo",
];

const TOOL_OPTIONS: LogSelectOption[] = [
  { value: "", label: "Все инструменты" },
  ...KNOWN_TOOLS.map((t) => ({ value: t, label: t })),
];

const STATUS_OPTIONS: LogSelectOption[] = [
  { value: "", label: "Любой статус" },
  { value: "ok", label: "Успех" },
  { value: "error", label: "Ошибка" },
];

function formatTs(tsMs: number): string {
  return new Date(tsMs).toLocaleString("ru-RU", {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

export function ToolCallLogModal({ projectRoot, onClose }: ToolCallLogModalProps) {
  const {
    rows,
    loading,
    error,
    clearing,
    search,
    setSearch,
    tool,
    setTool,
    status,
    setStatus,
    dateRange,
    setDateRange,
    onlyCurrentRepo,
    setOnlyCurrentRepo,
    canPrev,
    canNext,
    rangeLabel,
    prevPage,
    nextPage,
    clearLog,
  } = useToolCallLog(projectRoot);

  // Purely presentational: which row is unfolded, and whether the clear
  // button is showing its confirmation.
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [confirmingClear, setConfirmingClear] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const handleClear = async () => {
    if (await clearLog()) {
      setConfirmingClear(false);
      setExpandedId(null);
    }
  };

  return (
    <div className="tool-log-backdrop" role="presentation" onClick={onClose}>
      <div
        className="tool-log-dialog"
        role="dialog"
        aria-labelledby="tool-log-dialog-title"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="tool-log-header">
          <h2 className="tool-log-title" id="tool-log-dialog-title">
            Журнал вызовов инструментов
          </h2>
          <div className="tool-log-header-actions">
            {confirmingClear ? (
              <>
                <span className="tool-log-confirm-text">Удалить весь журнал?</span>
                <button
                  type="button"
                  className="tool-log-btn tool-log-btn-danger"
                  disabled={clearing}
                  onClick={() => void handleClear()}
                >
                  Да, очистить
                </button>
                <button
                  type="button"
                  className="tool-log-btn"
                  disabled={clearing}
                  onClick={() => setConfirmingClear(false)}
                >
                  Отмена
                </button>
              </>
            ) : (
              <button type="button" className="tool-log-btn" onClick={() => setConfirmingClear(true)}>
                Очистить журнал
              </button>
            )}
            <button type="button" className="tool-log-close" onClick={onClose} aria-label="Закрыть">
              ×
            </button>
          </div>
        </header>

        <div className="tool-log-filters">
          <input
            type="text"
            className="tool-log-search"
            placeholder="Поиск по инструменту или ошибке…"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
          />
          <LogSelect label="Инструмент" value={tool} options={TOOL_OPTIONS} onChange={setTool} />
          <LogSelect label="Статус" value={status} options={STATUS_OPTIONS} onChange={setStatus} />
          <LogSelect
            label="Период"
            value={dateRange}
            options={DATE_RANGE_OPTIONS}
            onChange={(value) => setDateRange(value as DateRangeOption)}
          />
          <label className="tool-log-checkbox-label">
            <input
              type="checkbox"
              checked={onlyCurrentRepo}
              disabled={!projectRoot}
              onChange={(event) => setOnlyCurrentRepo(event.target.checked)}
            />
            <span>Только текущий репозиторий</span>
          </label>
        </div>

        {error ? <div className="tool-log-error">{error}</div> : null}

        <div className="tool-log-body">
          <table className="tool-log-table">
            <thead>
              <tr>
                <th>Время</th>
                <th>Инструмент</th>
                <th>Репозиторий</th>
                <th>Статус</th>
                <th>Длительность</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <Fragment key={row.id}>
                  <tr
                    className={`tool-log-row${row.status === "error" ? " is-error" : ""}${
                      expandedId === row.id ? " is-expanded" : ""
                    }`}
                    onClick={() => setExpandedId(expandedId === row.id ? null : row.id)}
                  >
                    <td>{formatTs(row.tsMs)}</td>
                    <td>{row.tool}</td>
                    <td className="tool-log-repo" title={row.repoRoot}>
                      {row.repoRoot}
                    </td>
                    <td>
                      <span className={`tool-log-status tool-log-status-${row.status}`}>
                        {row.status === "ok" ? "успех" : "ошибка"}
                      </span>
                    </td>
                    <td>{row.durationMs} мс</td>
                  </tr>
                  {expandedId === row.id ? (
                    <tr className="tool-log-detail-row">
                      <td colSpan={5}>
                        <div className="tool-log-detail">
                          {row.source === "chat" ? (
                            <div className="tool-log-detail-meta">
                              Раунд {row.round ?? "—"} · {row.providerId ?? "—"} · {row.model ?? "—"}
                            </div>
                          ) : (
                            <div className="tool-log-detail-meta">Отдельный вызов (standalone)</div>
                          )}
                          {row.errorMessage ? (
                            <div className="tool-log-detail-error">{row.errorMessage}</div>
                          ) : null}
                          <div className="tool-log-detail-columns">
                            <div>
                              <div className="tool-log-detail-label">Аргументы</div>
                              <pre className="tool-log-detail-json">{formatJson(row.argsJson)}</pre>
                            </div>
                            {row.resultJson ? (
                              <div>
                                <div className="tool-log-detail-label">Результат</div>
                                <pre className="tool-log-detail-json">{formatJson(row.resultJson)}</pre>
                              </div>
                            ) : null}
                          </div>
                        </div>
                      </td>
                    </tr>
                  ) : null}
                </Fragment>
              ))}
              {!loading && rows.length === 0 ? (
                <tr>
                  <td colSpan={5} className="tool-log-empty">
                    Ничего не найдено
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
