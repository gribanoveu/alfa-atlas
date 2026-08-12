import { Editor } from "@monaco-editor/react";
import { useEffect, useState } from "react";
import { readProjectFileOrNone } from "../../lib/project";
import { monacoLanguageFor } from "../../lib/supportedFiles";
import { ATLAS_DARK_THEME_ID } from "../../monaco/asciidocLanguage";

type DeleteFileReviewProps = {
  docsRoot: string;
  path: string;
};

/** Read-only preview of a pending `deleteFile` call — the file's current
 * content, so the user sees what's about to be removed instead of judging
 * a bare path string. Same fetch/loading/error shape as `WriteFileDiffReview`,
 * but a plain `Editor` rather than `DiffEditor`: there's nothing to diff
 * against, only content to show. */
export function DeleteFileReview({ docsRoot, path }: DeleteFileReviewProps) {
  const [content, setContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(false);
    readProjectFileOrNone(docsRoot, path)
      .then((result) => {
        if (!cancelled) setContent(result ?? "");
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
      <Editor
        height="240px"
        theme={ATLAS_DARK_THEME_ID}
        language={monacoLanguageFor(path)}
        value={content ?? ""}
        options={{
          readOnly: true,
          automaticLayout: true,
          minimap: { enabled: false },
          scrollBeyondLastLine: false,
          wordWrap: "on",
          fontFamily: "'JetBrains Mono', ui-monospace, monospace",
          fontSize: 12,
        }}
      />
    </div>
  );
}
