import type * as Monaco from "monaco-editor";
import type { IDisposable } from "monaco-editor";
import { useEffect, useRef } from "react";
import {
  addCustomDictionaryWord,
  checkSpelling,
  spellcheckKindFor,
  suggestSpelling,
  type SpellcheckConfig,
  type SpellIssue,
} from "../lib/spellcheck";
import type { EditorTab } from "./useEditorTabs";

const OWNER = "spellcheck";
const DEBOUNCE_MS = 500;
const ADD_TO_DICTIONARY_COMMAND_ID = "spellcheck.addToDictionary";

function issueAtPosition(
  issues: SpellIssue[],
  line: number,
  column: number,
): SpellIssue | undefined {
  return issues.find(
    (issue) =>
      issue.line === line &&
      column >= issue.column &&
      column <= issue.column + issue.length,
  );
}

/**
 * Debounced Rust-side spellcheck for the active Monaco model: squiggly
 * markers via `setModelMarkers` (own owner, so they coexist with the
 * workspace-index diagnostics from `useMonacoDiagnostics`) plus a quick-fix
 * menu offering suggested corrections and "add to dictionary".
 */
export function useMonacoSpellcheck(
  monaco: typeof Monaco | null,
  editor: Monaco.editor.IStandaloneCodeEditor | null,
  activeTab: EditorTab | null,
  config: SpellcheckConfig,
) {
  const issuesRef = useRef<SpellIssue[]>([]);
  const runCheckRef = useRef<() => void>(() => {});

  // 1. Debounced check + markers — re-runs on content change and tab switch.
  useEffect(() => {
    if (!monaco || !editor || !activeTab) return;
    const model = editor.getModel();
    if (!model) return;

    let cancelled = false;

    const runCheck = async () => {
      if (!config.enabled) {
        issuesRef.current = [];
        monaco.editor.setModelMarkers(model, OWNER, []);
        return;
      }
      const text = model.getValue();
      const kind = spellcheckKindFor(activeTab.path);
      let issues: SpellIssue[];
      try {
        issues = await checkSpelling(text, kind);
      } catch {
        return;
      }
      if (cancelled || editor.getModel() !== model) return;

      issuesRef.current = issues;
      const markers: Monaco.editor.IMarkerData[] = issues.map((issue) => ({
        startLineNumber: issue.line,
        startColumn: issue.column,
        endLineNumber: issue.line,
        endColumn: issue.column + issue.length,
        message: `Возможно, слово «${issue.word}» написано с ошибкой`,
        severity: monaco.MarkerSeverity.Info,
        source: OWNER,
      }));
      monaco.editor.setModelMarkers(model, OWNER, markers);
    };

    runCheckRef.current = () => void runCheck();
    void runCheck();

    let timer: number | undefined;
    const disposable = editor.onDidChangeModelContent(() => {
      if (timer !== undefined) window.clearTimeout(timer);
      timer = window.setTimeout(() => void runCheck(), DEBOUNCE_MS);
    });

    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
      disposable.dispose();
      monaco.editor.setModelMarkers(model, OWNER, []);
    };
  }, [monaco, editor, activeTab, config]);

  // 2. "Add to dictionary" command — a global command with a stable id so
  // code actions across all checks can reference it.
  useEffect(() => {
    if (!monaco) return;
    const disposable = monaco.editor.registerCommand(
      ADD_TO_DICTIONARY_COMMAND_ID,
      (_accessor, word: string) => {
        void addCustomDictionaryWord(word).then(() => runCheckRef.current());
      },
    );
    return () => disposable.dispose();
  }, [monaco]);

  // 3. Code action provider — registered once, reads the latest issues from
  // a ref so it never needs to be re-registered as checks complete.
  useEffect(() => {
    if (!monaco) return;
    const disposers: IDisposable[] = [];

    disposers.push(
      monaco.languages.registerCodeActionProvider("*", {
        async provideCodeActions(model, range) {
          const issue = issueAtPosition(
            issuesRef.current,
            range.startLineNumber,
            range.startColumn,
          );
          if (!issue) return undefined;

          const wordRange: Monaco.IRange = {
            startLineNumber: issue.line,
            startColumn: issue.column,
            endLineNumber: issue.line,
            endColumn: issue.column + issue.length,
          };

          const suggestions = await suggestSpelling(issue.word).catch(
            () => [] as string[],
          );

          const actions: Monaco.languages.CodeAction[] = suggestions.map(
            (suggestion) => ({
              title: `Заменить на «${suggestion}»`,
              kind: "quickfix",
              edit: {
                edits: [
                  {
                    resource: model.uri,
                    textEdit: { range: wordRange, text: suggestion },
                    versionId: model.getVersionId(),
                  },
                ],
              },
            }),
          );

          actions.push({
            title: `Добавить «${issue.word}» в словарь`,
            kind: "quickfix",
            command: {
              id: ADD_TO_DICTIONARY_COMMAND_ID,
              title: "Добавить в словарь",
              arguments: [issue.word],
            },
          });

          return { actions, dispose: () => {} };
        },
      }),
    );

    return () => disposers.forEach((d) => d.dispose());
  }, [monaco]);
}
