import type {
  AbstractBlock,
  AbstractNode,
  Document,
  List,
  ListItem,
  Section,
} from "asciidoctor";

/**
 * Переэкспорт AST-типов asciidoctor для удобства компонентов рендера.
 * Импортируются через paths-маппинг в tsconfig.json (→ dist/types/index).
 *
 * `Table` не экспортируется из основного index-файла типов, поэтому в
 * табличном компоненте используется `AbstractBlock` с приведением к
 * runtime-форме `rows`/`columns`.
 */
export type {
  AbstractBlock,
  AbstractNode,
  Document,
  List,
  ListItem,
  Section,
};

/**
 * A table column node. Not exported from asciidoctor's main type index
 * (same reason as `AscTable` below), so it's typed structurally as an
 * `AbstractNode` — `getAttribute('colpcwidth')` is what `[cols="..."]`
 * resolves to (percentage width per column, e.g. `[cols="1,3,1"]` → 20/60/20).
 */
export type AscColumn = AbstractNode & { style: string | null };

/** Минимальная runtime-форма таблицы asciidoctor. */
export type AscTable = AbstractBlock & {
  rows: {
    head: AscCell[][];
    body: AscCell[][];
    foot: AscCell[][];
  };
  columns: AscColumn[];
  hasHeaderOption: boolean;
};

export type AscCell = {
  getText(): string | null;
  colspan: number;
  rowspan: number;
  getInnerDocument?: () => unknown;
};
