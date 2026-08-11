import type * as Monaco from "monaco-editor";
import { useEffect } from "react";
import type { IDisposable } from "monaco-editor";
import {
  findAnchors,
  getAttributes,
  getDocuments,
} from "../lib/workspaceIndex";
import { isUnderDocsRoot, relativizeToDocument, toDocsRelativePath } from "../lib/paths";
import { ASCIIDOC_LANGUAGE_ID } from "../monaco/asciidocLanguage";

const ADOC_LANGUAGE = ASCIIDOC_LANGUAGE_ID;

/**
 * `!`-command words mapped to AsciiDoc snippet templates. Every command
 * uses Monaco's snippet syntax (`${n:default}`) over its editable fields so
 * Tab/Shift+Tab walks through them after insertion, `$0` marks the final
 * cursor stop. Content mirrors the matching entries in `asciidocSnippets.ts`
 * — boilerplate (headers, `|===` fences, column specs) stays fixed, only
 * the fields a user would actually fill in become tab stops.
 */
type BangCommand = {
  command: string;
  detail: string;
  insertText: string;
};

const BANG_COMMANDS: BangCommand[] = [
  {
    command: "table",
    detail: "Таблица AsciiDoc (2 колонки, 1 строка)",
    insertText:
      '[cols="1,1"]\n' +
      "|===\n" +
      "| ${1:Колонка A} | ${2:Колонка B}\n" +
      "\n" +
      "| ${3:Значение 1} | ${4:Значение 2}\n" +
      "|===\n" +
      "$0",
  },
  {
    command: "request",
    detail: "Таблица входных параметров запроса",
    insertText:
      "== Входные параметры\n" +
      "\n" +
      '[cols="1,1,1,1,3,1"]\n' +
      "|===\n" +
      "| *Тип параметра*   | *Параметр* | *Формат* | *Обязательность* | *Описание* | *Варианты значений*\n" +
      "\n" +
      "|Метод            5+| ${1:POST}\n" +
      "|Endpoint         5+| ${2:corp-}\n" +
      "\n" +
      "| Header          \n" +
      "| A-userId     \n" +
      "| string\n" +
      "| required\n" +
      "| X-pin клиента, инициатора запроса\n" +
      "| XAAAAA\n" +
      "\n" +
      "| Header          \n" +
      "| A-userIp    \n" +
      "| string\n" +
      "| optional\n" +
      "| Ip-адресс клиента\n" +
      "| 64.233.165.113\n" +
      "\n" +
      "| Header          \n" +
      "| A-customerId  \n" +
      "| string\n" +
      "| required\n" +
      "| U-pin клиента, инициатора запроса\n" +
      "| UAAAAA\n" +
      "\n" +
      "| Header          \n" +
      "| A-projectId\n" +
      "| string\n" +
      "| required\n" +
      "| Идентификатор приложения инициатора запроса\n" +
      "| WOWTAX\n" +
      "\n" +
      "| Header          \n" +
      "| A-clientType\n" +
      "| string\n" +
      "| required\n" +
      "| Тип сервиса инициатора запроса\n" +
      "| FRONT\n" +
      "\n" +
      "| Header          \n" +
      "| A-channelId\n" +
      "| string\n" +
      "| required\n" +
      "| Идентификатор вызывающей системы (канала) NIB/ABM/BAAS\n" +
      "| NIB\n" +
      "\n" +
      "6+| Тело запроса\n" +
      "\n" +
      "| Body          \n" +
      "| ${3:fieldName}\n" +
      "| ${4:string}\n" +
      "| ${5:required}\n" +
      "| ${6:Описание поля}\n" +
      "| ${7:-}\n" +
      "|===\n" +
      "$0",
  },
  {
    command: "response",
    detail: "Таблица полей ответа",
    insertText:
      "== Поля ответа\n" +
      "\n" +
      '[cols="1,1,3,1"]\n' +
      "|===\n" +
      "| Параметр | Формат | Описание | Варианты значений\n" +
      "\n" +
      "| ${1:fieldName}\n" +
      "| ${2:string}\n" +
      "| ${3:description}\n" +
      "| ${4:values}\n" +
      "|===\n" +
      "$0",
  },
  {
    command: "validation",
    detail: "Таблица полей валидации",
    insertText:
      "== Поля валидации\n" +
      "\n" +
      '[cols="1,1,1"]\n' +
      "|===\n" +
      "| Параметр | Условие | Результат \n" +
      "\n" +
      "| ${1:param}\n" +
      "| ${2:condition}\n" +
      "| ${3:result}\n" +
      "|===\n" +
      "$0",
  },
  {
    command: "errors",
    detail: "Таблица кодов ошибок",
    insertText:
      "== Коды ошибок\n" +
      "\n" +
      '[cols="1,1,2,2"]\n' +
      "|===\n" +
      "| Type | Error Code | Message | Описание\n" +
      "\n" +
      "| ${1:ValidationException}\n" +
      "| ${2:validationError}\n" +
      "| ${3:Some of input parameters are incorrect}\n" +
      "| ${4:Входные параметры не прошли валидацию}\n" +
      "|===\n" +
      "$0",
  },
  {
    command: "json",
    detail: "Listing-блок с JSON",
    insertText:
      "[source,json]\n" +
      "----\n" +
      '{\n' +
      '  "${1:example}": "${2:value}"\n' +
      "}\n" +
      "----\n" +
      "$0",
  },
  {
    command: "note",
    detail: "Блок заметки",
    insertText: "NOTE: ${1:Текст заметки.}\n$0",
  },
  {
    command: "tip",
    detail: "Блок подсказки",
    insertText: "TIP: ${1:Полезная подсказка.}\n$0",
  },
  {
    command: "warning",
    detail: "Блок предупреждения",
    insertText: "WARNING: ${1:Текст предупреждения.}\n$0",
  },
  {
    command: "important",
    detail: "Блок важной информации",
    insertText: "IMPORTANT: ${1:Важная информация.}\n$0",
  },
];

/**
 * Derive the document id (relative path) from a Monaco model URI. The index
 * keys documents by relative path; model URIs carry that path (without a
 * leading slash) per the editor's `openFile` wiring.
 */
function documentIdFromModel(model: Monaco.editor.ITextModel): string {
  return model.uri.path.replace(/^\//, "");
}

function lineUpToCursor(
  model: Monaco.editor.ITextModel,
  position: Monaco.Position,
): string {
  return model.getValueInRange({
    startLineNumber: position.lineNumber,
    startColumn: 1,
    endLineNumber: position.lineNumber,
    endColumn: position.column,
  });
}

/**
 * Finds an in-progress `include::`/`image::`/`xref:` target the cursor is
 * currently sitting inside of, by looking for the *last* occurrence of
 * `keyword` before the cursor (not requiring it to sit immediately next to
 * the cursor) and returning everything typed since — `null` once whitespace
 * or `[` shows up, since that ends the path/target portion of the macro.
 *
 * This is what actually makes the suggestion survive edits, unlike a plain
 * `lineUpToCursor(...).endsWith(keyword)` check: `lastIndexOf` still finds
 * `keyword` no matter how much has been typed or deleted after it, so the
 * provider keeps firing (and Monaco keeps calling it, since `quickSuggestions`
 * re-queries providers on every keystroke, not just `triggerCharacters`) on
 * every forward *and* backward edit — including backspacing mid-path, which
 * is exactly where the old exact-match version went permanently blank (no
 * more `:` gets typed to re-trigger it via `triggerCharacters`).
 *
 * The returned `range` spans the whole in-progress target (from right after
 * `keyword` to the cursor), not a zero-width point at the cursor — so
 * accepting a suggestion *replaces* what's already been typed instead of
 * inserting next to it, which is what let a stale/duplicated path survive
 * before.
 */
function findOpenMacroTarget(
  model: Monaco.editor.ITextModel,
  position: Monaco.Position,
  keyword: string,
): { partial: string; range: Monaco.IRange } | null {
  const before = lineUpToCursor(model, position);
  const idx = before.lastIndexOf(keyword);
  if (idx === -1) return null;
  const partial = before.slice(idx + keyword.length);
  if (/[\s[]/.test(partial)) return null;
  return {
    partial,
    range: {
      startLineNumber: position.lineNumber,
      startColumn: position.column - partial.length,
      endLineNumber: position.lineNumber,
      endColumn: position.column,
    },
  };
}

/**
 * Registers five AsciiDoc completion providers on Monaco's `asciidoc`
 * language (`.adoc`/`.asciidoc` map to it in `supportedFiles.ts`) per spec
 * section 9: `include::`, `xref:`, `image::`, `{` for attributes, and a
 * `!`-triggered command menu (`!table`, `!request`, `!response`, ...) for
 * inserting AsciiDoc snippets.
 *
 * Each provider fetches fresh data from the index on trigger — O(1) lookups
 * per spec. `provideCompletionItems` may return a Promise, so async `invoke`
 * calls work natively.
 */
export function useMonacoCompletions(
  monaco: typeof Monaco | null,
  enabled: boolean,
  docsRoot: string | null,
  repoRoot: string | null,
) {
  useEffect(() => {
    if (!monaco || !enabled) return;
    const disposers: IDisposable[] = [];

    // Suggestions insert a path relative to the *current* file's directory
    // — matching how every include::/image::/xref: target in this codebase
    // is actually authored — not the index's repo-relative key. The index
    // itself covers the whole repository (`WorkspaceIndex::build` scans
    // from `repoRoot`), so it holds plenty of documents outside `docsRoot`
    // too — those can never actually be reached by one of these macros
    // (always resolved relative to `docsRoot`), so `isUnderDocsRoot` drops
    // them before they're ever offered as a completion.
    const docSuggestions = (
      model: Monaco.editor.ITextModel,
      docs: { relativePath: string }[],
      range: Monaco.IRange,
    ) => {
      const sourceDocsRelative = documentIdFromModel(model);
      return docs
        .filter((d) => !docsRoot || !repoRoot || isUnderDocsRoot(d.relativePath, repoRoot, docsRoot))
        .map((d) => {
          const docsRelative =
            docsRoot && repoRoot
              ? toDocsRelativePath(d.relativePath, repoRoot, docsRoot)
              : d.relativePath;
          const insertText = relativizeToDocument(docsRelative, sourceDocsRelative);
          return {
            label: insertText,
            kind: monaco.languages.CompletionItemKind.File,
            insertText,
            range,
          };
        });
    };

    // 1. include:: — list all documents. `findOpenMacroTarget` (not a plain
    // `endsWith` check) is what keeps this alive across edits — see its own
    // doc comment.
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: [":"],
        async provideCompletionItems(model, position) {
          const open = findOpenMacroTarget(model, position, "include::");
          if (!open) return null;
          const docs = await getDocuments();
          return { suggestions: docSuggestions(model, docs, open.range) };
        },
      }),
    );

    // 2. xref: — documents; after `doc#` — anchors of that doc.
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: [":", "#"],
        async provideCompletionItems(model, position) {
          const open = findOpenMacroTarget(model, position, "xref:");
          if (!open) return null;
          const hashIdx = open.partial.indexOf("#");
          if (hashIdx === -1) {
            const docs = await getDocuments();
            return { suggestions: docSuggestions(model, docs, open.range) };
          }
          const docId = open.partial.slice(0, hashIdx);
          if (!docId) return null;
          const anchorRange: Monaco.IRange = {
            startLineNumber: position.lineNumber,
            startColumn: open.range.startColumn + hashIdx + 1,
            endLineNumber: position.lineNumber,
            endColumn: position.column,
          };
          const anchors = await findAnchors(docId);
          return {
            suggestions: anchors.map((a) => ({
              label: a.id,
              kind: monaco.languages.CompletionItemKind.Reference,
              insertText: a.id,
              range: anchorRange,
            })),
          };
        },
      }),
    );

    // 3. image:: — list documents (images live alongside docs in the index).
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: [":"],
        async provideCompletionItems(model, position) {
          const open = findOpenMacroTarget(model, position, "image::");
          if (!open) return null;
          const docs = await getDocuments();
          return { suggestions: docSuggestions(model, docs, open.range) };
        },
      }),
    );

    // 4. `{` — list attributes defined in the active document.
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: ["{"],
        async provideCompletionItems(model, position) {
          const open = findOpenMacroTarget(model, position, "{");
          // `}` closes the attribute reference — same as `[` for
          // include::/image::, past that point this isn't the name anymore.
          if (!open || open.partial.includes("}")) return null;
          const docId = documentIdFromModel(model);
          const attrs = await getAttributes(docId);
          return {
            suggestions: attrs.map((a) => ({
              label: a.name,
              kind: monaco.languages.CompletionItemKind.Variable,
              insertText: a.name,
              detail: a.value || undefined,
              range: open.range,
            })),
          };
        },
      }),
    );

    // 5. `!` — command menu inserting AsciiDoc snippets (table, request
    // params, response fields, admonitions, ...). `!` is used instead of
    // `/` because `/` already appears constantly in ordinary AsciiDoc text
    // (URLs, image::/xref::/include:: paths, `//` comments), which would
    // otherwise pop the suggestion widget on nearly every slash. All
    // matching commands are returned on trigger; Monaco filters the list
    // client-side as the user keeps typing the command word.
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: ["!"],
        provideCompletionItems(model, position) {
          const match = lineUpToCursor(model, position).match(/!(\w*)$/);
          if (!match) return null;
          const range: Monaco.IRange = {
            startLineNumber: position.lineNumber,
            startColumn: position.column - match[0].length,
            endLineNumber: position.lineNumber,
            endColumn: position.column,
          };
          return {
            suggestions: BANG_COMMANDS.map((c) => ({
              label: `!${c.command}`,
              kind: monaco.languages.CompletionItemKind.Snippet,
              detail: c.detail,
              filterText: `!${c.command}`,
              insertText: c.insertText,
              insertTextRules:
                monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
              range,
            })),
          };
        },
      }),
    );

    return () => disposers.forEach((d) => d.dispose());
  }, [monaco, enabled]);
}
