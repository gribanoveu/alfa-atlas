import { DiffEditor } from "@monaco-editor/react";
import { useEffect, useState } from "react";
import { readProjectFileOrNone } from "../../lib/project";
import { monacoLanguageFor } from "../../lib/supportedFiles";
import { ATLAS_DARK_THEME_ID } from "../../monaco/asciidocLanguage";

type WriteFileDiffReviewProps = {
  docsRoot: string;
  path: string;
  content: string;
};

/** Read-only preview of a pending `writeFile` call — current on-disk
 * content (empty for a file that doesn't exist yet) vs. the proposed
 * content, so the user can judge the change before approving. Unlike
 * `GitFileDiffModal`'s editable revert surface, both panes here are
 * read-only: this is a preview before the write happens, not a place to
 * hand-edit the proposed content. */
export function WriteFileDiffReview({ docsRoot, path, content }: WriteFileDiffReviewProps) {
  const [original, setOriginal] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(false);
    readProjectFileOrNone(docsRoot, path)
      .then((result) => {
        if (!cancelled) setOriginal(result ?? "");
      })
      .catch(() => {
        if (!cancelled) setError(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [docsRoot, path]);

  if (loading) {
    return <div className="tool-approval-diff-placeholder">Загрузка текущего содержимого…</div>;
  }
  if (error) {
    return (
      <div className="tool-approval-diff-placeholder tool-approval-diff-error">
        Не удалось загрузить текущее содержимое файла
      </div>
    );
  }

  return (
    <div className="tool-approval-diff">
      <DiffEditor
        height="240px"
        theme={ATLAS_DARK_THEME_ID}
        language={monacoLanguageFor(path)}
        original={original ?? ""}
        modified={content}
        options={{
          readOnly: true,
          originalEditable: false,
          renderSideBySide: true,
          automaticLayout: true,
          minimap: { enabled: false },
          scrollBeyondLastLine: false,
          wordWrap: "on",
          fontFamily: "'JetBrains Mono', ui-monospace, monospace",
          fontSize: 12,
          renderOverviewRuler: false,
        }}
      />
    </div>
  );
}
