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

  return (
    <table className="asc-table">
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
