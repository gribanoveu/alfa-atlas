import { DiffEditor } from "@monaco-editor/react";
import { useEffect, useMemo, useState } from "react";
import type { FileEdit } from "../../lib/aiTools";
import { applyEditsExact, type EditApplyResult } from "../../lib/editApply";
import { readProjectFileOrNone } from "../../lib/project";
import { monacoLanguageFor } from "../../lib/supportedFiles";
import { ATLAS_DARK_THEME_ID } from "../../monaco/asciidocLanguage";

type EditFileDiffReviewProps = {
  docsRoot: string;
  path: string;
  edits: FileEdit[];
};

function truncate(text: string, max = 80): string {
  return text.length > max ? `${text.slice(0, max)}…` : text;
}

/** `notFound`/`ambiguous` still leave room for the server-side fast-apply
 * fallback to reconcile the edit (see `apply_edits` in
 * `src-tauri/src/services/ai_tools.rs`), so the message says the preview is
 * unavailable rather than that the call will fail. `overlap` gets no such
 * caveat — the backend never attempts fast-apply for it, so claiming it
 * might still work would be misleading. */
function describeApplyFailure(result: Extract<EditApplyResult, { ok: false }>): string {
  switch (result.reason) {
    case "notFound":
      return `Не удалось найти текст для замены: «${truncate(result.old)}». Предпросмотр недоступен — при подтверждении бэкенд может всё же сопоставить правку автоматически.`;
    case "ambiguous":
      return `Текст для замены встречается ${result.count} раза — неоднозначно: «${truncate(result.old)}». Предпросмотр недоступен — при подтверждении бэкенд может всё же сопоставить правку автоматически.`;
    case "overlap":
      return "Несколько правок пересекаются в одном участке файла — это всегда завершится ошибкой, правки нужно исправить.";
  }
}

/** Read-only preview of a pending `editFile` call — applies `edits` to the
 * current on-disk content in memory (`applyEditsExact`, mirroring the
 * backend's own exact-match pass) and diffs original vs. result, so the
 * user can judge the change before approving instead of reading raw
 * `old`/`new` pairs. Same shape as `WriteFileDiffReview`, kept as a
 * separate component rather than a shared one: the two diverge in props
 * (`content` vs `edits`) and in failure surface (this one can also fail to
 * even *compute* a diff if the edits don't match cleanly). */
export function EditFileDiffReview({ docsRoot, path, edits }: EditFileDiffReviewProps) {
  const [original, setOriginal] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [fetchError, setFetchError] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setFetchError(false);
    readProjectFileOrNone(docsRoot, path)
      .then((result) => {
        if (!cancelled) setOriginal(result ?? "");
      })
      .catch(() => {
        if (!cancelled) setFetchError(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [docsRoot, path]);

  const applied = useMemo(
    () => (original === null ? null : applyEditsExact(original, edits)),
    [original, edits],
  );

  if (loading) {
    return <div className="tool-approval-diff-placeholder">Загрузка текущего содержимого…</div>;
  }
  if (fetchError) {
    return (
      <div className="tool-approval-diff-placeholder tool-approval-diff-error">
        Не удалось загрузить текущее содержимое файла
      </div>
    );
  }
  if (applied && !applied.ok) {
    return (
      <div className="tool-approval-diff-placeholder tool-approval-diff-error">
        {describeApplyFailure(applied)}
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
        modified={applied && applied.ok ? applied.content : (original ?? "")}
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
