import { useCallback, useEffect, useMemo, useState } from "react";
import { TreeNode } from "../StructuredDataPreview/TreeNode";
import { collectPaths, type StructuredValue } from "../StructuredDataPreview/structuredDataUtils";
import "../StructuredDataPreview/StructuredDataPreview.css";
import "./OpenApiExplorer.css";

type ResponseBodyViewProps = {
  body: string;
};

/** Renders a response body as a collapsible JSON tree, reusing the same
 * `TreeNode` component the app already uses for `.json`/`.yaml` file
 * previews — so an API response looks the same as browsing a spec file.
 * Falls back to raw text when the body isn't valid JSON (plain text, HTML,
 * XML, empty). */
export function ResponseBodyView({ body }: ResponseBodyViewProps) {
  const parsed = useMemo<StructuredValue | undefined>(() => {
    const trimmed = body.trim();
    if (trimmed === "") return undefined;
    try {
      return JSON.parse(trimmed) as StructuredValue;
    } catch {
      return undefined;
    }
  }, [body]);

  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set(["root"]));

  // Re-open the top level whenever a new response body arrives.
  useEffect(() => {
    setExpanded(new Set(["root"]));
  }, [parsed]);

  const toggle = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const expandAll = useCallback(() => {
    if (parsed === undefined) return;
    setExpanded(collectPaths(parsed, "root"));
  }, [parsed]);

  const collapseAll = useCallback(() => setExpanded(new Set()), []);

  if (parsed === undefined) {
    return <pre className="oas-try-response-body">{body}</pre>;
  }

  return (
    <div className="oas-try-response-json">
      <div className="struct-toolbar">
        <button type="button" className="struct-toolbar-btn" onClick={expandAll}>
          Развернуть всё
        </button>
        <button type="button" className="struct-toolbar-btn" onClick={collapseAll}>
          Свернуть всё
        </button>
      </div>
      <div className="struct-tree">
        <TreeNode data={parsed} path="root" expanded={expanded} onToggle={toggle} />
      </div>
    </div>
  );
}
