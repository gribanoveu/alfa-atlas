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

/** Минимальная runtime-форма таблицы asciidoctor. */
export type AscTable = AbstractBlock & {
  rows: {
    head: AscCell[][];
    body: AscCell[][];
    foot: AscCell[][];
  };
  columns: { style: string | null }[];
  hasHeaderOption: boolean;
};

export type AscCell = {
  getText(): string | null;
  colspan: number;
  rowspan: number;
  getInnerDocument?: () => unknown;
};
