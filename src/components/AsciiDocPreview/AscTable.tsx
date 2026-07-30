import type { AscTable, AscCell } from "./types";
import { InlineHtml } from "./InlineHtml";

/**
 * Таблица AsciiDoc. asciidoctor уже разделил строки на head/body/foot
 * и применил inline-подстановки к тексту ячеек.
 */
export function AscTable({ table }: { table: AscTable }) {
  const { head, body, foot } = table.rows;
  const hasHead = head.length > 0;
  const hasFoot = foot.length > 0;
  const columns = table.columns;
  // Mirrors asciidoctor's own HTML5 converter: the `autowidth` table option
  // drops all explicit widths (fit-content), otherwise each column gets the
  // percentage width `[cols="..."]` resolved it to (equal split when the
  // attribute is absent), unless that column itself opts into autowidth.
  const tableAutowidth = table.hasOption("autowidth");

  return (
    <table className="asc-table">
      {columns.length > 0 ? (
        <colgroup>
          {columns.map((column, ci) =>
            tableAutowidth || column.hasOption("autowidth") ? (
              <col key={ci} />
            ) : (
              <col
                key={ci}
                style={{ width: `${column.getAttribute("colpcwidth")}%` }}
              />
            ),
          )}
        </colgroup>
      ) : null}
      {hasHead ? (
        <thead>
          {head.map((row, ri) => (
            <tr key={`h${ri}`}>
              {row.map((cell, ci) => (
                <th key={ci}>
                  <InlineHtml html={cell.getText()} />
                </th>
              ))}
            </tr>
          ))}
        </thead>
      ) : null}
      <tbody>
        {body.map((row, ri) => (
          <tr key={`b${ri}`}>
            {row.map((cell, ci) => (
              <td key={ci} colSpan={cell.colspan > 1 ? cell.colspan : undefined} rowSpan={cell.rowspan > 1 ? cell.rowspan : undefined}>
                <InlineHtml html={cell.getText()} />
              </td>
            ))}
          </tr>
        ))}
      </tbody>
      {hasFoot ? (
        <tfoot>
          {foot.map((row, ri) => (
            <tr key={`f${ri}`}>
              {row.map((cell, ci) => (
                <td key={ci}>
                  <InlineHtml html={cell.getText()} />
                </td>
              ))}
            </tr>
          ))}
        </tfoot>
      ) : null}
    </table>
  );
}

export type { AscCell };
