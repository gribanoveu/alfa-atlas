import type * as Monaco from "monaco-editor";
import { useCallback, useEffect, useRef, useState } from "react";
import { resolveRelativeToDocument, toDocsRelativePath, toRepoRelativePath } from "../lib/paths";
import { findAnchors } from "../lib/workspaceIndex";
import type { useEditorTabs } from "./useEditorTabs";
import type { useProject } from "./useProject";
import type { useWorkspaceIndex } from "./useWorkspaceIndex";
import type { useWorkspaceLayout } from "./useWorkspaceLayout";

/**
 * Resolve an xref `href` (`path#anchor`, `path`, or `#anchor`) against the
 * docs-relative `sourcePath` of the document that contains the link. Returns
 * a `{ path, anchor }` pair where `path` is docs-relative (suitable for
 * `editor.openFile`) and `anchor` may be `null`.
 *
 * When `href` has no path component (just `#anchor`), the target is the
 * current document — `path` is `sourcePath` unchanged.
 */
function resolveXrefHref(
  href: string,
  sourcePath: string,
): { path: string; anchor: string | null } {
  // Strip any `./`/`../`-style relative segments against the source file's
  // directory. We don't use `URL` because these hrefs are not real URLs
  // (no scheme) and Tauri webview may absolutize them oddly.
  const hashIdx = href.indexOf("#");
  const pathPart = hashIdx >= 0 ? href.slice(0, hashIdx) : href;
  const anchor = hashIdx >= 0 ? href.slice(hashIdx + 1) : null;

  if (!pathPart) {
    return { path: sourcePath, anchor: anchor ?? null };
  }

  return { path: resolveRelativeToDocument(pathPart, sourcePath), anchor: anchor ?? null };
}

type Deps = {
  editor: ReturnType<typeof useEditorTabs>;
  project: ReturnType<typeof useProject>;
  layout: ReturnType<typeof useWorkspaceLayout>;
  workspaceIndex: ReturnType<typeof useWorkspaceIndex>;
  /** `null` until Monaco loads; the Ctrl+Click opener re-registers once it
   * becomes available. */
  monacoInstance: typeof Monaco | null;
};

/** Opening a document *at a place* — a diagnostic, a search hit, an anchor,
 * an xref target — and telling the editor to scroll there.
 *
 * The scroll is a `revealRequest` carrying an incrementing id rather than a
 * plain line number, because the same location can be requested twice in a
 * row (clicking the same problem again) and a prop that compares equal would
 * be ignored the second time.
 *
 * Every path here fails quietly. A broken `include::`/`xref:` target is a
 * documentation bug the user already sees in Problems; surfacing a raw
 * filesystem error on top of it would only add noise. */
export function useDocNavigation({
  editor,
  project,
  layout,
  workspaceIndex,
  monacoInstance,
}: Deps) {
  const [revealRequest, setRevealRequest] = useState<{
    id: number;
    line: number;
    column: number;
    severity: "error" | "warning";
  } | null>(null);
  const revealCounter = useRef(0);
  const openDiagnostic = useCallback(
    async (documentId: string, line: number, column: number) => {
      // Если Problems panel был свёрнут — раскрываем его (как в IDE: клик по
      // проблеме не должен прятать сам список).
      if (!layout.bottomTool) {
        layout.setBottomToolId("problems");
      }

      const severity: "error" | "warning" = (() => {
        const found = workspaceIndex.diagnostics.find(
          (d) =>
            d.document === documentId &&
            d.line === line &&
            d.column === column,
        );
        return found?.severity === "warning" ? "warning" : "error";
      })();

      const reveal = () => {
        revealCounter.current += 1;
        setRevealRequest({
          id: revealCounter.current,
          line,
          column,
          severity,
        });
      };

      if (project.docsRoot && project.repoRoot) {
        // `documentId` — это repo-relative ключ индекса (например
        // `src/docs/asciidoc/foo.adoc`), а `editor.openFile` ожидает путь
        // относительно docsRoot (`foo.adoc`). Считаем относительный суффикс.
        const rel = toDocsRelativePath(
          documentId,
          project.repoRoot,
          project.docsRoot,
        );
        try {
          await editor.openFile(rel);
          reveal();
          return;
        } catch {
          // Путь не открылся — ниже общий fallback.
        }
      }
      try {
        await editor.openFile(documentId);
        reveal();
      } catch {
        // Файл не существует (битый include) — тихо игнорируем, не показывая
        // сырую os-ошибку. Пользователь уже видит диагностику в Problems.
      }
    },
    [
      editor,
      layout,
      project.docsRoot,
      project.repoRoot,
      workspaceIndex.diagnostics,
    ],
  );
  const openDocsSearchHit = useCallback(
    async (path: string, line: number) => {
      try {
        await editor.openFile(path);
        revealCounter.current += 1;
        setRevealRequest({
          id: revealCounter.current,
          line,
          column: 1,
          severity: "warning",
        });
      } catch {
        // Missing / unsupported file — leave the search overlay open.
      }
    },
    [editor],
  );
  /**
   * Открывает `relPath` (относительно docsRoot) и, если передан `anchor`,
   * прокручивает редактор к его строке через `findAnchors`. Общая для клика
   * по xref-ссылке в превью (`openXref` ниже) и для Ctrl+Click «перейти к
   * файлу» из самого Monaco (см. `registerEditorOpener` ниже).
   */
  const openDocumentReference = useCallback(
    async (relPath: string, anchor: string | null) => {
      try {
        await editor.openFile(relPath);
      } catch {
        // Файл не существует (битая ссылка) — тихо игнорируем; диагностику
        // пользователь уже видит в Problems, если она была построена.
        return;
      }

      if (!anchor) return;
      const repoRoot = project.repoRoot;
      const docsRoot = project.docsRoot;
      if (!repoRoot || !docsRoot) return;

      const documentId = toRepoRelativePath(relPath, repoRoot, docsRoot);
      try {
        const anchors = await findAnchors(documentId);
        const hit = anchors.find((a) => a.id === anchor);
        if (!hit) return;
        revealCounter.current += 1;
        setRevealRequest({
          id: revealCounter.current,
          line: hit.line,
          column: hit.column,
          severity: "warning",
        });
      } catch {
        // Индекс недоступен или документ не проиндексирован — оставляем
        // пользователя на открытой файле без прокрутки.
      }
    },
    [editor, project.repoRoot, project.docsRoot],
  );
  /**
   * Клик по xref-ссылке в превью AsciiDoc. Распарсивает href (`path#anchor`,
   * `path`, `#anchor`) и делегирует открытие+прокрутку `openDocumentReference`.
   */
  const openXref = useCallback(
    async (href: string) => {
      const sourcePath = editor.activeTab?.path;
      if (!sourcePath) return;
      // Внешние URL — не наша зона ответственности, пропускаем.
      if (/^https?:\/\//i.test(href) || href.startsWith("mailto:")) return;

      const { path: relPath, anchor } = resolveXrefHref(href, sourcePath);
      await openDocumentReference(relPath, anchor);
    },
    [editor, openDocumentReference],
  );
  // Ctrl+Click (Cmd+Click на macOS) на цель include::/image::/xref: —
  // useMonacoDefinitions.ts резолвит макрос в Uri (docs-relative путь,
  // якорь — в fragment), а дальше Monaco зовёт этот «opener», потому что
  // сам standalone-редактор не умеет открывать чужие ресурсы.
  useEffect(() => {
    if (!monacoInstance) return;
    const disposer = monacoInstance.editor.registerEditorOpener({
      openCodeEditor(_source, resource) {
        const path = resource.path.replace(/^\//, "");
        if (!path) return false;
        void openDocumentReference(path, resource.fragment || null);
        return true;
      },
    });
    return () => disposer.dispose();
  }, [monacoInstance, openDocumentReference]);
  return {
    revealRequest,
    openDiagnostic,
    openDocsSearchHit,
    openDocumentReference,
    openXref,
  };
}
