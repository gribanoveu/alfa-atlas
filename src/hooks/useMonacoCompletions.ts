import type * as Monaco from "monaco-editor";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import type { IDisposable } from "monaco-editor";
import {
  buildDocPathSuggestions,
  documentsToPathEntries,
  INCLUDE_DOC_KINDS,
  XREF_DOC_KINDS,
  type DocPathSuggestion,
} from "../lib/docPathSuggestions";
import { resolveRelativeToDocument } from "../lib/paths";
import { listImageFiles, resolveAssetPath, type ImageFileEntry } from "../lib/project";
import {
  findAnchors,
  getAttributes,
  getDocuments,
  INDEX_EVENT_CHANNEL,
  type Document,
  type DocumentType,
  type IndexEvent,
} from "../lib/workspaceIndex";
import { ASCIIDOC_LANGUAGE_ID } from "../monaco/asciidocLanguage";

const ADOC_LANGUAGE = ASCIIDOC_LANGUAGE_ID;

/** Max bytes for an `image::` completion preview (data URI). */
const IMAGE_PREVIEW_MAX_BYTES = 512 * 1024;

const RETRIGGER_SUGGEST_COMMAND = {
  id: "editor.action.triggerSuggest",
  title: "Re-trigger suggest",
} as const;

type AtlasImageCompletion = Monaco.languages.CompletionItem & {
  atlasDocsRelative?: string;
};

async function blobToDataUri(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("FileReader failed"));
    reader.readAsDataURL(blob);
  });
}

async function loadImagePreviewDataUri(
  docsRoot: string,
  docsRelativePath: string,
): Promise<string | null> {
  try {
    const abs = await resolveAssetPath(docsRoot, docsRelativePath);
    const res = await fetch(convertFileSrc(abs));
    if (!res.ok) return null;
    const blob = await res.blob();
    if (blob.size === 0 || blob.size > IMAGE_PREVIEW_MAX_BYTES) return null;
    return await blobToDataUri(blob);
  } catch {
    return null;
  }
}

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

/** Exported for the sync test against `ASCIIDOC_SNIPPETS` — stripped of its
 * tab stops, each command must reproduce the snippet catalog's template
 * verbatim, so the two hand-kept copies cannot drift apart silently. */
export const BANG_COMMANDS: BangCommand[] = [
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
    detail: "Входные параметры REST-метода",
    insertText:
      "=== Входные параметры\n" +
      "\n" +
      '[cols="1,1,1,1,3,1"]\n' +
      "|===\n" +
      "| *Тип параметра* | *Параметр* | *Формат* | *Обязательность* | *Описание* | *Варианты значений*\n" +
      "\n" +
      "|Метод 5+| ${1:POST}\n" +
      "|Endpoint 5+| ${2:https://\\{host\\}/<сервис>/<путь>}\n" +
      "\n" +
      "| Header\n" +
      "| A-userId\n" +
      "| string (length 6 or 9)\n" +
      "| required\n" +
      "| Идентификатор УЛ интернет-банка. Заполняется входным параметром A-userId\n" +
      "| XAAAAA\n" +
      "\n" +
      "| Header\n" +
      "| A-customerId\n" +
      "| string (length 6 or 9)\n" +
      "| required\n" +
      "| Идентификатор клиента. Заполняется входным параметром A-customerId\n" +
      "| UAAAAA\n" +
      "\n" +
      "| Header\n" +
      "| A-userIp\n" +
      "| string\n" +
      "| optional\n" +
      "| IP УЛ.\n" +
      "| 64.233.165.113\n" +
      "\n" +
      "| Header\n" +
      "| A-projectId\n" +
      "| string\n" +
      "| optional\n" +
      "| Идентификатор приложения (модуля приложения) потребителя.\n" +
      "| CORP-<ПРОЕКТ>\n" +
      "\n" +
      "| Header\n" +
      "| A-clientType\n" +
      "| string\n" +
      "| required\n" +
      "| Тип сервиса инициатора запроса.\n" +
      "| FRONT\n" +
      "\n" +
      "| Header\n" +
      "| A-channelId\n" +
      "| string\n" +
      "| required\n" +
      "| Идентификатор вызывающей системы (канала).\n" +
      "| NIB\n" +
      "\n" +
      "6+| Тело запроса\n" +
      "\n" +
      "| Body\n" +
      "| ${3:fieldName}\n" +
      "| ${4:string}\n" +
      "| ${5:required}\n" +
      "| ${6:Описание поля и чем оно заполняется}\n" +
      "| ${7:-}\n" +
      "|===\n" +
      "$0",
  },
  {
    command: "thrift",
    detail: "Входные параметры Thrift-метода (userData)",
    insertText:
      "=== Входные параметры\n" +
      "\n" +
      '[cols="1,1,1,3"]\n' +
      "|===\n" +
      "| *Параметр* | *Формат* | *Обязательность* | *Описание*\n" +
      "\n" +
      "|Endpoint 3+| ${1:\\{host\\}/<сервис>/tapi}\n" +
      "\n" +
      "| userData\n" +
      "| struct\n" +
      "| required\n" +
      "| Данные пользователя\n" +
      "\n" +
      "| userData.id\n" +
      "| string\n" +
      "| required\n" +
      "| Идентификатор пользователя (xpin/acus)\n" +
      "\n" +
      "| userData.authorizedApplicationId\n" +
      "| string\n" +
      "| required\n" +
      "| Идентификатор приложения\n" +
      "\n" +
      "| userData.ip\n" +
      "| string\n" +
      "| required\n" +
      "| IP-адрес пользователя\n" +
      "\n" +
      "| userData.customerId\n" +
      "| string\n" +
      "| required\n" +
      "| Идентификатор клиента\n" +
      "\n" +
      "| ${2:fieldName}\n" +
      "| ${3:string}\n" +
      "| ${4:required}\n" +
      "| ${5:Описание поля и чем оно заполняется}\n" +
      "|===\n" +
      "$0",
  },
  {
    command: "response",
    detail: "Выходные параметры",
    insertText:
      "=== Выходные параметры\n" +
      "\n" +
      '[cols="1,1,3,1"]\n' +
      "|===\n" +
      "| *Параметр* | *Формат* | *Описание* | *Варианты значений*\n" +
      "\n" +
      "| ${1:fieldName}\n" +
      "| ${2:string}\n" +
      "| ${3:Описание поля и источник значения}\n" +
      "| ${4:-}\n" +
      "|===\n" +
      "$0",
  },
  {
    command: "validation",
    detail: "Валидация входных параметров",
    insertText:
      "=== Валидация входных параметров\n" +
      "\n" +
      '[cols="1,2,3"]\n' +
      "|===\n" +
      "| *Параметр* | *Условие* | *Результат*\n" +
      "\n" +
      ".2+|${1:A-userId}\n" +
      "|Параметр имеет значение null или пусто\n" +
      '|Вернуть исключение с http code 400 (Bad Request), указанием на некорректный параметр - "A-userId не указан", type = VALIDATION_ERROR и code = INCORRECT_CONTRACT\n' +
      "|Длина значения не равна 6 символам либо символы не являются алфавитно-числовыми\n" +
      '|Вернуть исключение с http code 400 (Bad Request), указанием на некорректный параметр - "A-userId должен содержать 6 алфавитно-цифровых символов", type = VALIDATION_ERROR и code = INCORRECT_CONTRACT\n' +
      "|===\n" +
      "$0",
  },
  {
    command: "errors",
    detail: "Обработка ошибок (include + таблица кодов)",
    insertText:
      "== Обработка ошибок\n" +
      "Описание приведено по ссылке ниже.\n" +
      "\n" +
      "include::../CompositeException.adoc[]\n" +
      "\n" +
      "*Коды ошибок*\n" +
      "\n" +
      '[cols="2,2,1,1"]\n' +
      "|===\n" +
      "| *Условие* | *Описание* | *Type* | *Code*\n" +
      "\n" +
      "| ${1:Шаг 1. Валидация. Не указан параметр}\n" +
      "| ${2:A-userId не указан}\n" +
      "| ${3:VALIDATION_ERROR}\n" +
      "| ${4:INCORRECT_CONTRACT}\n" +
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
 * Path suggestions for `include::`/`xref:`/`image::` are pre-filtered in
 * `buildDocPathSuggestions` (Monaco's word filter splits on `/` and would
 * otherwise empty the list mid-path). Folder items re-trigger suggest after
 * accept. `image::` lists real assets via `list_image_files` and attaches a
 * data-URI preview in `resolveCompletionItem`. Document/image lists are
 * cached and invalidated on workspace-index events.
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

    let docsCache: Document[] | null = null;
    let docsInflight: Promise<Document[]> | null = null;
    let imagesCache: ImageFileEntry[] | null = null;
    let imagesInflight: Promise<ImageFileEntry[]> | null = null;

    const loadDocuments = async (): Promise<Document[]> => {
      if (docsCache) return docsCache;
      if (!docsInflight) {
        docsInflight = getDocuments()
          .then((docs) => {
            docsCache = docs;
            return docs;
          })
          .finally(() => {
            docsInflight = null;
          });
      }
      return docsInflight;
    };

    const loadImages = async (): Promise<ImageFileEntry[]> => {
      if (!docsRoot) return [];
      if (imagesCache) return imagesCache;
      if (!imagesInflight) {
        imagesInflight = listImageFiles(docsRoot)
          .then((images) => {
            imagesCache = images;
            return images;
          })
          .finally(() => {
            imagesInflight = null;
          });
      }
      return imagesInflight;
    };

    const invalidateCaches = () => {
      docsCache = null;
      imagesCache = null;
    };

    let unlistenIndex: (() => void) | null = null;
    let cancelled = false;
    void listen<IndexEvent>(INDEX_EVENT_CHANNEL, (event) => {
      if (cancelled) return;
      const kind = event.payload.kind;
      if (
        kind === "indexUpdated" ||
        kind === "indexBuildingFinished" ||
        kind === "indexBuildingStarted"
      ) {
        invalidateCaches();
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlistenIndex = fn;
    });

    const mapPathSuggestions = (
      built: DocPathSuggestion[],
      range: Monaco.IRange,
      opts?: {
        imageDocsRelative?: (insertText: string) => string;
        /** Close a file path so the AsciiDoc macro is valid (`path[]`). */
        fileClose?: "brackets" | "xref-snippet";
      },
    ): Monaco.languages.CompletionItem[] =>
      built.map((s) => {
        let insertText = s.insertText;
        let insertTextRules: Monaco.languages.CompletionItemInsertTextRule | undefined;
        if (s.kind === "file" && opts?.fileClose === "brackets") {
          insertText = `${s.insertText}[]`;
        } else if (s.kind === "file" && opts?.fileClose === "xref-snippet") {
          insertText = `${s.insertText}$0[]`;
          insertTextRules =
            monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet;
        }
        const base: Monaco.languages.CompletionItem = {
          label: s.label,
          detail: s.detail,
          kind:
            s.kind === "folder"
              ? monaco.languages.CompletionItemKind.Folder
              : monaco.languages.CompletionItemKind.File,
          insertText,
          filterText: s.filterText,
          sortText: s.sortText,
          range,
          ...(insertTextRules ? { insertTextRules } : {}),
          ...(s.kind === "folder" ? { command: { ...RETRIGGER_SUGGEST_COMMAND } } : {}),
        };
        if (s.kind === "file" && opts?.imageDocsRelative) {
          (base as AtlasImageCompletion).atlasDocsRelative = opts.imageDocsRelative(
            s.insertText,
          );
        }
        return base;
      });

    const docPathSuggestions = (
      model: Monaco.editor.ITextModel,
      docs: Document[],
      partial: string,
      range: Monaco.IRange,
      kinds: readonly DocumentType[] | undefined,
      fileClose: "brackets" | "xref-snippet",
    ) => {
      const built = buildDocPathSuggestions({
        entries: documentsToPathEntries(docs),
        sourceDocsRelative: documentIdFromModel(model),
        docsRoot,
        repoRoot,
        partial,
        kinds,
        excludeSelf: true,
        pathSpace: "repo",
      });
      return mapPathSuggestions(built, range, { fileClose });
    };

    // 1. include:: — path list. `findOpenMacroTarget` keeps this alive across
    // edits; `/` is a trigger so directory prefixes re-open the widget.
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: [":", "/"],
        async provideCompletionItems(model, position) {
          const open = findOpenMacroTarget(model, position, "include::");
          if (!open) return null;
          const docs = await loadDocuments();
          return {
            suggestions: docPathSuggestions(
              model,
              docs,
              open.partial,
              open.range,
              INCLUDE_DOC_KINDS,
              "brackets",
            ),
            incomplete: true,
          };
        },
      }),
    );

    // 2. xref: — documents; after `doc#` — anchors of that doc.
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: [":", "/", "#"],
        async provideCompletionItems(model, position) {
          const open = findOpenMacroTarget(model, position, "xref:");
          if (!open) return null;
          const hashIdx = open.partial.indexOf("#");
          if (hashIdx === -1) {
            const docs = await loadDocuments();
            return {
              suggestions: docPathSuggestions(
                model,
                docs,
                open.partial,
                open.range,
                XREF_DOC_KINDS,
                "xref-snippet",
              ),
              incomplete: true,
            };
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
              insertText: `${a.id}[]`,
              range: anchorRange,
            })),
          };
        },
      }),
    );

    // 3. image:: — real image files under docsRoot + folder prefixes;
    // resolveCompletionItem attaches a data-URI preview for focused files.
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: [":", "/"],
        async provideCompletionItems(model, position) {
          const open = findOpenMacroTarget(model, position, "image::");
          if (!open || !docsRoot) return null;
          const images = await loadImages();
          const sourceDocsRelative = documentIdFromModel(model);
          const built = buildDocPathSuggestions({
            entries: images.map((img) => ({
              relativePath: img.relativePath,
              fileName: img.fileName,
            })),
            sourceDocsRelative,
            docsRoot,
            repoRoot,
            partial: open.partial,
            pathSpace: "docs",
            excludeSelf: false,
          });
          return {
            suggestions: mapPathSuggestions(built, open.range, {
              fileClose: "brackets",
              imageDocsRelative: (insertText) =>
                resolveRelativeToDocument(insertText, sourceDocsRelative),
            }),
            incomplete: true,
          };
        },
        async resolveCompletionItem(item) {
          const docsRel = (item as AtlasImageCompletion).atlasDocsRelative;
          if (!docsRel || !docsRoot) return item;
          const dataUri = await loadImagePreviewDataUri(docsRoot, docsRel);
          if (!dataUri) return item;
          const label =
            typeof item.label === "string" ? item.label : item.label.label;
          item.documentation = {
            value: `![${label}](${dataUri})`,
          };
          return item;
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

    return () => {
      cancelled = true;
      if (unlistenIndex) unlistenIndex();
      disposers.forEach((d) => d.dispose());
    };
  }, [monaco, enabled, docsRoot, repoRoot]);
}
