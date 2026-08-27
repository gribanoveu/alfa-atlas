import { Minus, Plus } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import {
  cloneEditableTable,
  distributeColumnWidths,
  serializeAsciidocTable,
  tableColumnCount,
  type EditableCell,
  type EditableRow,
  type EditableTable,
  type RowLayout,
} from "../../lib/asciidocTableModel";
import "../Welcome/CloneRepoModal.css";
import "./AsciidocTableEditorModal.css";

type CellRef = { rowIndex: number; cellIndex: number };

type AsciidocTableEditorModalProps = {
  initialTable: EditableTable;
  onSave: (newSource: string) => void;
  onCancel: () => void;
};

const DEFAULT_COLUMN_WIDTH_PX = 120;
const MIN_COLUMN_WIDTH_PX = 48;
const ROW_CONTROL_WIDTH_PX = 26;
const COL_CONTROL_STRIP_HEIGHT_PX = 20;

function availableDataWidth(containerWidth: number): number {
  return Math.max(0, containerWidth - ROW_CONTROL_WIDTH_PX);
}

function computeBoundaryLefts(widths: number[]): number[] {
  const boundaries: number[] = [];
  let offset = 0;
  for (let i = 0; i < widths.length - 1; i++) {
    offset += widths[i];
    boundaries.push(offset);
  }
  return boundaries;
}

function applyPairDelta(left: number, right: number, delta: number): [number, number] {
  let nextLeft = left + delta;
  let nextRight = right - delta;

  if (nextLeft < MIN_COLUMN_WIDTH_PX) {
    nextRight -= MIN_COLUMN_WIDTH_PX - nextLeft;
    nextLeft = MIN_COLUMN_WIDTH_PX;
  }
  if (nextRight < MIN_COLUMN_WIDTH_PX) {
    nextLeft -= MIN_COLUMN_WIDTH_PX - nextRight;
    nextRight = MIN_COLUMN_WIDTH_PX;
  }

  return [nextLeft, nextRight];
}

function clearBodyDragStyles() {
  document.body.style.userSelect = "";
  document.body.style.cursor = "";
}

function normalizeSpan(value: number): number {
  return value > 1 ? value : 1;
}

function rowWidth(row: EditableRow): number {
  return row.cells.reduce((sum, cell) => sum + normalizeSpan(cell.colspan), 0);
}

function emptyCell(): EditableCell {
  return { text: "", colspan: 1, rowspan: 1 };
}

function initialColumnWidths(table: EditableTable): number[] {
  const count = Math.max(1, tableColumnCount(table));
  return Array.from({ length: count }, () => DEFAULT_COLUMN_WIDTH_PX);
}

function lastBodyRowLayout(rows: EditableRow[]): RowLayout {
  for (let i = rows.length - 1; i >= 0; i--) {
    if (rows[i].section === "body") return rows[i].layout;
  }
  return "vertical";
}

function findCellAtLogicalColumn(
  row: EditableRow,
  logicalCol: number,
): { cellIndex: number; cell: EditableCell; logicalStart: number } | null {
  let pos = 0;
  for (let i = 0; i < row.cells.length; i++) {
    const cell = row.cells[i];
    const span = normalizeSpan(cell.colspan);
    if (logicalCol >= pos && logicalCol < pos + span) {
      return { cellIndex: i, cell, logicalStart: pos };
    }
    pos += span;
  }
  return null;
}

function insertRowAfter(table: EditableTable, rowIndex: number): EditableTable {
  const cols = Math.max(1, tableColumnCount(table));
  const next = cloneEditableTable(table);
  const anchor = next.rows[rowIndex];
  const section = anchor?.section ?? "body";
  const layout =
    section === "head" ? "horizontal" : (anchor?.layout ?? lastBodyRowLayout(next.rows));
  next.rows.splice(rowIndex + 1, 0, {
    section,
    layout,
    cells: Array.from({ length: cols }, () => emptyCell()),
  });
  return next;
}

function removeRowAt(table: EditableTable, rowIndex: number): EditableTable {
  if (table.rows.length <= 1) return table;
  const next = cloneEditableTable(table);
  next.rows.splice(rowIndex, 1);
  return next;
}

function insertColumnAfter(table: EditableTable, colIndex: number): EditableTable {
  const next = cloneEditableTable(table);
  for (const row of next.rows) {
    const found = findCellAtLogicalColumn(row, colIndex);
    if (!found) {
      row.cells.push(emptyCell());
      continue;
    }

    const span = normalizeSpan(found.cell.colspan);
    const logicalEnd = found.logicalStart + span - 1;
    if (colIndex === logicalEnd) {
      row.cells.splice(found.cellIndex + 1, 0, emptyCell());
      continue;
    }

    const leftSpan = colIndex - found.logicalStart + 1;
    const rightSpan = span - leftSpan;
    found.cell.colspan = leftSpan;
    if (rightSpan > 1) {
      row.cells.splice(found.cellIndex + 1, 0, emptyCell(), {
        ...emptyCell(),
        colspan: rightSpan,
      });
    } else {
      row.cells.splice(found.cellIndex + 1, 0, emptyCell());
    }
  }
  return next;
}

function removeColumnAt(table: EditableTable, colIndex: number): EditableTable {
  if (tableColumnCount(table) <= 1) return table;
  const next = cloneEditableTable(table);
  for (const row of next.rows) {
    const found = findCellAtLogicalColumn(row, colIndex);
    if (!found) continue;
    const span = normalizeSpan(found.cell.colspan);
    if (span > 1) {
      found.cell.colspan = span - 1;
    } else {
      row.cells.splice(found.cellIndex, 1);
    }
  }
  return next;
}

function mergeCellsRight(table: EditableTable, ref: CellRef): EditableTable {
  const row = table.rows[ref.rowIndex];
  if (!row || ref.cellIndex >= row.cells.length - 1) return table;
  const next = cloneEditableTable(table);
  const targetRow = next.rows[ref.rowIndex];
  const left = targetRow.cells[ref.cellIndex];
  const right = targetRow.cells[ref.cellIndex + 1];
  left.colspan = normalizeSpan(left.colspan) + normalizeSpan(right.colspan);
  left.rowspan = Math.max(normalizeSpan(left.rowspan), normalizeSpan(right.rowspan));
  if (right.text.trim()) {
    left.text = left.text.trim()
      ? `${left.text.trim()} ${right.text.trim()}`
      : right.text.trim();
  }
  targetRow.cells.splice(ref.cellIndex + 1, 1);
  return next;
}

function mergeCellsDown(table: EditableTable, ref: CellRef): EditableTable {
  const row = table.rows[ref.rowIndex];
  const below = table.rows[ref.rowIndex + 1];
  if (!row || !below || ref.cellIndex >= row.cells.length || ref.cellIndex >= below.cells.length) {
    return table;
  }
  const next = cloneEditableTable(table);
  const top = next.rows[ref.rowIndex].cells[ref.cellIndex];
  const bottom = next.rows[ref.rowIndex + 1].cells[ref.cellIndex];
  if (normalizeSpan(top.colspan) !== normalizeSpan(bottom.colspan)) return table;
  top.rowspan = normalizeSpan(top.rowspan) + normalizeSpan(bottom.rowspan);
  if (bottom.text.trim()) {
    top.text = top.text.trim()
      ? `${top.text.trim()} ${bottom.text.trim()}`
      : bottom.text.trim();
  }
  next.rows[ref.rowIndex + 1].cells.splice(ref.cellIndex, 1);
  if (next.rows[ref.rowIndex + 1].cells.length === 0) {
    next.rows.splice(ref.rowIndex + 1, 1);
  }
  return next;
}

function splitCell(table: EditableTable, ref: CellRef): EditableTable {
  const row = table.rows[ref.rowIndex];
  const cell = row?.cells[ref.cellIndex];
  if (!cell) return table;
  if (normalizeSpan(cell.colspan) === 1 && normalizeSpan(cell.rowspan) === 1) return table;

  const next = cloneEditableTable(table);
  const target = next.rows[ref.rowIndex].cells[ref.cellIndex];
  const colspan = normalizeSpan(target.colspan);
  const rowspan = normalizeSpan(target.rowspan);
  target.colspan = 1;
  target.rowspan = 1;

  for (let c = 1; c < colspan; c++) {
    next.rows[ref.rowIndex].cells.splice(ref.cellIndex + c, 0, emptyCell());
  }

  for (let r = 1; r < rowspan; r++) {
    const insertAt = ref.rowIndex + r;
    if (!next.rows[insertAt]) {
      next.rows.splice(insertAt, 0, {
        section: row.section,
        layout: row.layout,
        cells: Array.from({ length: rowWidth(row) }, () => emptyCell()),
      });
    }
    for (let c = 0; c < colspan; c++) {
      if (!next.rows[insertAt].cells[ref.cellIndex + c]) {
        next.rows[insertAt].cells.splice(ref.cellIndex + c, 0, emptyCell());
      }
    }
  }

  return next;
}

function updateCellText(
  table: EditableTable,
  ref: CellRef,
  text: string,
): EditableTable {
  const next = cloneEditableTable(table);
  const cell = next.rows[ref.rowIndex]?.cells[ref.cellIndex];
  if (!cell) return table;
  cell.text = text;
  return next;
}

type CellTextareaProps = {
  value: string;
  onChange: (value: string) => void;
  onSelect: () => void;
};

function CellTextarea({ value, onChange, onSelect }: CellTextareaProps) {
  const ref = useRef<HTMLTextAreaElement>(null);

  const syncHeight = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, []);

  useLayoutEffect(() => {
    syncHeight();
  }, [value, syncHeight]);

  useLayoutEffect(() => {
    const el = ref.current;
    const cell = el?.parentElement;
    if (!cell) return;
    const observer = new ResizeObserver(() => syncHeight());
    observer.observe(cell);
    return () => observer.disconnect();
  }, [syncHeight]);

  return (
    <textarea
      ref={ref}
      className="asciidoc-table-editor-cell-input"
      value={value}
      rows={1}
      onChange={(event) => {
        onChange(event.target.value);
        syncHeight();
      }}
      onFocus={onSelect}
      onClick={onSelect}
    />
  );
}

function TableControlButton({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      className="asciidoc-table-editor-control-btn"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      {children}
    </button>
  );
}

export function AsciidocTableEditorModal({
  initialTable,
  onSave,
  onCancel,
}: AsciidocTableEditorModalProps) {
  const [table, setTable] = useState(() => cloneEditableTable(initialTable));
  const [selected, setSelected] = useState<CellRef | null>(null);
  const [columnWidths, setColumnWidths] = useState(() => initialColumnWidths(initialTable));
  const [resizingColIndex, setResizingColIndex] = useState<number | null>(null);
  const columnWidthsRef = useRef(columnWidths);
  const resizeRef = useRef<{ colIndex: number; lastX: number } | null>(null);
  const gridWrapRef = useRef<HTMLDivElement>(null);
  const filledInitialRef = useRef(false);

  const [containerWidth, setContainerWidth] = useState(0);

  columnWidthsRef.current = columnWidths;

  const columnCount = useMemo(() => tableColumnCount(table), [table]);

  const columnBoundaryLefts = useMemo(
    () => computeBoundaryLefts(columnWidths),
    [columnWidths],
  );

  const totalTableWidthPx = useMemo(
    () => columnWidths.reduce((sum, width) => sum + width, 0),
    [columnWidths],
  );

  const tableContentWidthPx = totalTableWidthPx + ROW_CONTROL_WIDTH_PX;

  const hostWidthPx = tableContentWidthPx;

  useLayoutEffect(() => {
    const el = gridWrapRef.current;
    if (!el) return;
    const update = () => {
      const width = el.clientWidth;
      if (width > 0) setContainerWidth(width);
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    if (containerWidth <= 0 || columnCount <= 0) return;
    if (filledInitialRef.current) return;
    filledInitialRef.current = true;
    setColumnWidths(
      distributeColumnWidths(
        columnCount,
        availableDataWidth(containerWidth),
        table.colsAttribute,
      ),
    );
  }, [containerWidth, columnCount, table.colsAttribute]);

  useEffect(() => {
    if (!filledInitialRef.current || containerWidth <= 0) return;
    setColumnWidths((prev) => {
      if (prev.length === columnCount) return prev;
      const prevTotal = prev.reduce((sum, width) => sum + width, 0);
      const target = Math.max(availableDataWidth(containerWidth), prevTotal);
      return distributeColumnWidths(columnCount, target, table.colsAttribute);
    });
  }, [columnCount, containerWidth, table.colsAttribute]);

  useEffect(() => {
    if (resizingColIndex === null) return;

    const handlePointerMove = (event: PointerEvent) => {
      const drag = resizeRef.current;
      if (!drag) return;

      const delta = event.clientX - drag.lastX;
      drag.lastX = event.clientX;
      if (delta === 0) return;

      const widths = columnWidthsRef.current;
      const left = widths[drag.colIndex] ?? DEFAULT_COLUMN_WIDTH_PX;
      const right = widths[drag.colIndex + 1] ?? DEFAULT_COLUMN_WIDTH_PX;
      const [nextLeft, nextRight] = applyPairDelta(left, right, delta);

      const next = [...widths];
      next[drag.colIndex] = nextLeft;
      next[drag.colIndex + 1] = nextRight;
      columnWidthsRef.current = next;
      setColumnWidths(next);
    };

    const finishDrag = () => {
      if (resizeRef.current === null) return;
      resizeRef.current = null;
      setResizingColIndex(null);
      clearBodyDragStyles();
    };

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", finishDrag);
    window.addEventListener("pointercancel", finishDrag);

    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", finishDrag);
      window.removeEventListener("pointercancel", finishDrag);
      if (resizeRef.current !== null) {
        finishDrag();
      }
    };
  }, [resizingColIndex]);

  const handleResizePointerDown = useCallback(
    (colIndex: number, event: ReactPointerEvent<HTMLDivElement>) => {
      event.preventDefault();
      event.stopPropagation();
      if (colIndex >= columnWidths.length - 1) return;

      resizeRef.current = { colIndex, lastX: event.clientX };
      setResizingColIndex(colIndex);
      document.body.style.userSelect = "none";
      document.body.style.cursor = "col-resize";
    },
    [columnWidths.length],
  );

  const handleCellChange = useCallback((ref: CellRef, text: string) => {
    setTable((prev) => updateCellText(prev, ref, text));
  }, []);

  const handleSelectCell = useCallback((ref: CellRef) => {
    setSelected(ref);
  }, []);

  const handleInsertRowAfter = useCallback((rowIndex: number) => {
    setTable((prev) => insertRowAfter(prev, rowIndex));
  }, []);

  const handleRemoveRowAt = useCallback((rowIndex: number) => {
    setTable((prev) => {
      const next = removeRowAt(prev, rowIndex);
      if (next === prev) return prev;
      setSelected((current) => {
        if (current === null) return null;
        if (current.rowIndex === rowIndex) return null;
        if (current.rowIndex > rowIndex) {
          return { ...current, rowIndex: current.rowIndex - 1 };
        }
        return current;
      });
      return next;
    });
  }, []);

  const handleInsertColumnAfter = useCallback((colIndex: number) => {
    setTable((prev) => insertColumnAfter(prev, colIndex));
  }, []);

  const handleRemoveColumnAt = useCallback((colIndex: number) => {
    setTable((prev) => {
      const next = removeColumnAt(prev, colIndex);
      if (next === prev) return prev;
      setSelected((current) => {
        if (current === null) return null;
        const found = findCellAtLogicalColumn(prev.rows[current.rowIndex], colIndex);
        if (found && found.cellIndex === current.cellIndex && normalizeSpan(found.cell.colspan) === 1) {
          return null;
        }
        return current;
      });
      return next;
    });
  }, []);

  const handleSave = () => {
    onSave(serializeAsciidocTable(table));
  };

  const canMergeRight =
    selected !== null &&
    selected.cellIndex < (table.rows[selected.rowIndex]?.cells.length ?? 0) - 1;

  const canMergeDown =
    selected !== null &&
    selected.rowIndex < table.rows.length - 1 &&
    selected.cellIndex < (table.rows[selected.rowIndex + 1]?.cells.length ?? 0);

  const canSplit =
    selected !== null &&
    (() => {
      const cell = table.rows[selected.rowIndex]?.cells[selected.cellIndex];
      if (!cell) return false;
      return normalizeSpan(cell.colspan) > 1 || normalizeSpan(cell.rowspan) > 1;
    })();

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onClick={onCancel}
    >
      <div
        className="clone-modal asciidoc-table-editor-modal"
        role="dialog"
        aria-labelledby="asciidoc-table-editor-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="asciidoc-table-editor-title">
          Редактор таблицы AsciiDoc
        </div>

        <div className="asciidoc-table-editor-toolbar">
          <span className="asciidoc-table-editor-toolbar-hint">
            Объединение ячеек — для выбранной ячейки
          </span>
          <button
            type="button"
            className="clone-modal-btn"
            disabled={!canMergeRight || selected === null}
            onClick={() => selected && setTable(mergeCellsRight(table, selected))}
          >
            Объединить →
          </button>
          <button
            type="button"
            className="clone-modal-btn"
            disabled={!canMergeDown || selected === null}
            onClick={() => selected && setTable(mergeCellsDown(table, selected))}
          >
            Объединить ↓
          </button>
          <button
            type="button"
            className="clone-modal-btn"
            disabled={!canSplit || selected === null}
            onClick={() => selected && setTable(splitCell(table, selected))}
          >
            Разъединить
          </button>
        </div>

        <div className="asciidoc-table-editor-scroll" ref={gridWrapRef}>
        <div className="asciidoc-table-editor-grid-wrap">
          <div
            className="asciidoc-table-editor-table-host"
            style={{
              ...(hostWidthPx > 0 ? { width: `${hostWidthPx}px` } : {}),
              ["--table-col-control-height" as string]: `${COL_CONTROL_STRIP_HEIGHT_PX}px`,
            }}
          >
            <div className="asciidoc-table-editor-col-resizers" aria-hidden>
              {columnBoundaryLefts.map((left, colIndex) => (
                <div
                  key={colIndex}
                  className={
                    resizingColIndex === colIndex
                      ? "asciidoc-table-editor-col-resizer is-active"
                      : "asciidoc-table-editor-col-resizer"
                  }
                  style={{ left: `${ROW_CONTROL_WIDTH_PX + left}px` }}
                  onPointerDown={(event) => handleResizePointerDown(colIndex, event)}
                />
              ))}
            </div>
            <table
              className="asciidoc-table-editor-grid"
              style={
                tableContentWidthPx > 0
                  ? { width: `${tableContentWidthPx}px` }
                  : undefined
              }
            >
              <colgroup>
                <col style={{ width: `${ROW_CONTROL_WIDTH_PX}px` }} />
                {columnWidths.map((width, colIndex) => (
                  <col key={colIndex} style={{ width: `${width}px` }} />
                ))}
              </colgroup>
              <thead>
                <tr className="asciidoc-table-editor-control-row">
                  <th className="asciidoc-table-editor-corner" scope="col" aria-hidden />
                  {Array.from({ length: columnCount }, (_, colIndex) => (
                    <th key={colIndex} className="asciidoc-table-editor-col-control" scope="col">
                      <div className="asciidoc-table-editor-col-control-inner">
                        <TableControlButton
                          label={`Добавить столбец после ${colIndex + 1}`}
                          onClick={() => handleInsertColumnAfter(colIndex)}
                        >
                          <Plus size={11} strokeWidth={2} aria-hidden />
                        </TableControlButton>
                        <TableControlButton
                          label={`Удалить столбец ${colIndex + 1}`}
                          disabled={columnCount <= 1}
                          onClick={() => handleRemoveColumnAt(colIndex)}
                        >
                          <Minus size={11} strokeWidth={2} aria-hidden />
                        </TableControlButton>
                      </div>
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
              {table.rows.map((row, rowIndex) => (
                <tr key={rowIndex}>
                  <td className="asciidoc-table-editor-row-control">
                    <div className="asciidoc-table-editor-row-control-inner">
                      <TableControlButton
                        label={`Добавить строку после ${rowIndex + 1}`}
                        onClick={() => handleInsertRowAfter(rowIndex)}
                      >
                        <Plus size={12} strokeWidth={2} aria-hidden />
                      </TableControlButton>
                      <TableControlButton
                        label={`Удалить строку ${rowIndex + 1}`}
                        disabled={table.rows.length <= 1}
                        onClick={() => handleRemoveRowAt(rowIndex)}
                      >
                        <Minus size={12} strokeWidth={2} aria-hidden />
                      </TableControlButton>
                    </div>
                  </td>
                  {row.cells.map((cell, cellIndex) => {
                    const isSelected =
                      selected?.rowIndex === rowIndex &&
                      selected?.cellIndex === cellIndex;
                    const Tag = row.section === "head" ? "th" : "td";
                    return (
                      <Tag
                        key={cellIndex}
                        colSpan={normalizeSpan(cell.colspan)}
                        rowSpan={normalizeSpan(cell.rowspan)}
                        className={
                          isSelected
                            ? "asciidoc-table-editor-cell selected"
                            : "asciidoc-table-editor-cell"
                        }
                        onClick={(event) => {
                          handleSelectCell({ rowIndex, cellIndex });
                          const textarea = event.currentTarget.querySelector("textarea");
                          if (
                            textarea instanceof HTMLTextAreaElement &&
                            event.target !== textarea
                          ) {
                            textarea.focus();
                          }
                        }}
                      >
                        <CellTextarea
                          value={cell.text}
                          onChange={(text) =>
                            handleCellChange({ rowIndex, cellIndex }, text)
                          }
                          onSelect={() => handleSelectCell({ rowIndex, cellIndex })}
                        />
                      </Tag>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
          </div>
        </div>
        </div>

        <div className="clone-modal-actions">
          <button type="button" className="clone-modal-btn" onClick={onCancel}>
            Отмена
          </button>
          <button type="button" className="clone-modal-btn primary" onClick={handleSave}>
            Сохранить
          </button>
        </div>
      </div>
    </div>
  );
}
