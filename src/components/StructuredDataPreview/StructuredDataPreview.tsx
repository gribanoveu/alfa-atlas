import { useCallback, useMemo, useState } from "react";
import type * as Monaco from "monaco-editor";
import "../AsciiDocPreview/AsciiDocPreview.css";
import { TreeNode } from "./TreeNode";
import {
  collectPaths,
  parseStructuredData,
} from "./structuredDataUtils";
import "./StructuredDataPreview.css";

type StructuredDataPreviewProps = {
  content: string;
  filePath: string | null;
  docsRoot: string | null;
  monaco: typeof Monaco | null;
};

export function StructuredDataPreview({
  content,
  filePath,
}: StructuredDataPreviewProps) {
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(
    () => new Set(),
  );

  const { data, error } = useMemo(
    () => parseStructuredData(content, filePath),
    [content, filePath],
  );

  const toggle = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const expandAll = useCallback(() => {
    if (data === null) return;
    setExpanded(collectPaths(data, "root"));
  }, [data]);

  const collapseAll = useCallback(() => {
    setExpanded(new Set());
  }, []);

  if (error) {
    return (
      <div className="asc-preview asc-preview-error">
        <div className="asc-preview-error-title">Ошибка парсинга</div>
        <pre className="asc-preview-error-detail">{error}</pre>
      </div>
    );
  }

  if (data === null) {
    return <div className="asc-preview asc-preview-empty">Нет содержимого</div>;
  }

  return (
    <div className="struct-preview">
      <div className="struct-toolbar">
        <button
          type="button"
          className="struct-toolbar-btn"
          onClick={expandAll}
        >
          Развернуть всё
        </button>
        <button
          type="button"
          className="struct-toolbar-btn"
          onClick={collapseAll}
        >
          Свернуть всё
        </button>
      </div>
      <div className="struct-tree">
        <TreeNode
          data={data}
          path="root"
          expanded={expanded}
          onToggle={toggle}
        />
      </div>
    </div>
  );
}
