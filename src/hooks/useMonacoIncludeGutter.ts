import type * as Monaco from "monaco-editor";
import { useEffect, useRef } from "react";
import {
  findAllMacroTargets,
  resolveMacroTargetDocsRelative,
  type MacroTarget,
} from "../lib/asciidocReferences";

/**
 * Click-to-navigate gutter icon for `include::`/`image::`/`xref:` lines —
 * the mouse-driven counterpart to `useMonacoDefinitions.ts`'s Ctrl+Click
 * "go to definition" (same target resolution, `resolveMacroTargetDocsRelative`,
 * shared by both). A line only gets the icon once its macro target actually
 * resolves to a real document — a broken reference (already flagged by
 * `useMonacoDiagnostics.ts`'s own glyph-margin error icon on that same line)
 * gets no icon, since clicking it couldn't navigate anywhere anyway, and
 * this keeps the two features from both wanting the same glyph-margin slot.
 *
 * Same native-decoration approach as `useMonacoDiagnostics.ts` (a
 * `codicon`-font `glyphMarginClassName`, no custom SVG) and the same
 * `onMouseDown` + `MouseTargetType.GUTTER_GLYPH_MARGIN` click pattern
 * `useGitGutter.ts` already established for a clickable gutter affordance.
 */
export function useMonacoIncludeGutter(
  monaco: typeof Monaco | null,
  editor: Monaco.editor.IStandaloneCodeEditor | null,
  docsRoot: string | null,
  repoRoot: string | null,
  onNavigate: ((docsRelativePath: string, anchor: string | null) => void) | undefined,
) {
  const decorationsRef = useRef<string[]>([]);
  const lineTargetsRef = useRef<Map<number, MacroTarget>>(new Map());
  const onNavigateRef = useRef(onNavigate);
  onNavigateRef.current = onNavigate;

  useEffect(() => {
    if (!monaco || !editor || !docsRoot || !repoRoot) return;

    let cancelled = false;
    let debounceHandle: ReturnType<typeof setTimeout> | null = null;

    const clearDecorations = () => {
      lineTargetsRef.current = new Map();
      decorationsRef.current = editor.deltaDecorations(decorationsRef.current, []);
    };

    const rescan = async () => {
      const model = editor.getModel();
      if (!model || model.isDisposed()) {
        clearDecorations();
        return;
      }

      const sourceDocsRelative = model.uri.path.replace(/^\//, "");
      const candidates: { line: number; target: MacroTarget }[] = [];
      for (let line = 1; line <= model.getLineCount(); line++) {
        const targets = findAllMacroTargets(model.getLineContent(line));
        if (targets.length > 0) candidates.push({ line, target: targets[0] });
      }

      const resolved = await Promise.all(
        candidates.map(async ({ line, target }) => ({
          line,
          target,
          docsRelative: await resolveMacroTargetDocsRelative(
            target,
            sourceDocsRelative,
            repoRoot,
            docsRoot,
          ),
        })),
      );
      if (cancelled || editor.getModel() !== model || model.isDisposed()) return;

      const hits = resolved.filter((r) => r.docsRelative !== null);
      const nextLineTargets = new Map<number, MacroTarget>(hits.map((h) => [h.line, h.target]));
      lineTargetsRef.current = nextLineTargets;

      decorationsRef.current = editor.deltaDecorations(
        decorationsRef.current,
        hits.map(({ line }) => ({
          range: new monaco.Range(line, 1, line, 1),
          options: {
            isWholeLine: false,
            glyphMarginClassName: "codicon codicon-go-to-file asciidoc-include-gutter-icon",
            glyphMarginHoverMessage: { value: "Перейти к файлу (клик по иконке)" },
          },
        })),
      );
    };

    const scheduleRescan = () => {
      if (debounceHandle) clearTimeout(debounceHandle);
      debounceHandle = setTimeout(() => void rescan(), 200);
    };

    scheduleRescan();
    const contentDisposable = editor.onDidChangeModelContent(scheduleRescan);
    const modelDisposable = editor.onDidChangeModel(scheduleRescan);

    const mouseDownDisposable = editor.onMouseDown((event) => {
      if (event.target.type !== monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN) return;
      const line = event.target.position?.lineNumber;
      if (!line) return;
      const target = lineTargetsRef.current.get(line);
      if (!target) return;

      event.event.preventDefault();
      event.event.stopPropagation();

      const model = editor.getModel();
      if (!model) return;
      const sourceDocsRelative = model.uri.path.replace(/^\//, "");
      void resolveMacroTargetDocsRelative(target, sourceDocsRelative, repoRoot, docsRoot).then(
        (resolvedDocsRelative) => {
          if (resolvedDocsRelative) onNavigateRef.current?.(resolvedDocsRelative, target.anchor);
        },
      );
    });

    // Only ever *sets* the pointer cursor, never resets it — `useGitGutter`
    // registers its own `onMouseMove` first (declared earlier in
    // `Editor.tsx`) and already resets to the default cursor on every move
    // that isn't over one of ITS gutter targets; since Monaco's listeners
    // fire in registration order, that reset always runs immediately before
    // this one, so leaving the non-matching branch as a no-op here means
    // this handler only ever overrides that default when it genuinely has
    // something clickable to show — it never fights the other hook for the
    // final cursor value.
    const mouseMoveDisposable = editor.onMouseMove((event) => {
      if (event.target.type !== monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN) return;
      const line = event.target.position?.lineNumber;
      if (line === undefined || !lineTargetsRef.current.has(line)) return;
      const dom = editor.getDomNode();
      if (dom) dom.style.cursor = "pointer";
    });

    return () => {
      cancelled = true;
      if (debounceHandle) clearTimeout(debounceHandle);
      contentDisposable.dispose();
      modelDisposable.dispose();
      mouseDownDisposable.dispose();
      mouseMoveDisposable.dispose();
      clearDecorations();
    };
  }, [monaco, editor, docsRoot, repoRoot]);
}
