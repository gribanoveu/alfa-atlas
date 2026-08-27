import type { ConversationMode } from "./aiTools";
import { basename, REQUEST_FROM_CURL_PROMPT_PREFIX } from "./assistantConfig";
import type { EditorTabKind, EditorTabOrigin } from "../hooks/useEditorTabs";

export type EditorActionContext = {
  path: string;
  basename: string;
  content: string;
  tabKind: EditorTabKind;
  tabOrigin: EditorTabOrigin;
  llmReady: boolean;
};

export type EditorActionInput =
  | { kind: "curl"; title: string; placeholder: string }
  | { kind: "text"; title: string; placeholder: string; multiline?: boolean }
  | { kind: "none" };

export type EditorContextAction = {
  id: string;
  label: string;
  when: (ctx: EditorActionContext) => boolean;
  input: EditorActionInput;
  conversationMode?: ConversationMode;
  buildPrompt: (ctx: EditorActionContext, inputValue?: string) => string;
};

export function matchesBasename(name: string, expected: string): boolean {
  return name.localeCompare(expected, undefined, { sensitivity: "accent" }) === 0;
}

export function editorActionContextFromTab(
  tab: {
    path: string;
    content: string;
    kind: EditorTabKind;
    origin: EditorTabOrigin;
  },
  llmReady: boolean,
): EditorActionContext {
  return {
    path: tab.path,
    basename: basename(tab.path),
    content: tab.content,
    tabKind: tab.kind,
    tabOrigin: tab.origin,
    llmReady,
  };
}

const EDITOR_CONTEXT_ACTIONS: EditorContextAction[] = [
  {
    id: "request-from-curl",
    label: "Заполнить по примеру curl",
    when: (ctx) =>
      ctx.llmReady &&
      ctx.tabKind === "text" &&
      ctx.tabOrigin === "project" &&
      matchesBasename(ctx.basename, "request.adoc"),
    input: {
      kind: "curl",
      title: "Пример curl-запроса",
      placeholder: 'curl -X POST "https://example/api" -H "Content-Type: application/json" -d \'{"key": "value"}\'',
    },
    conversationMode: "agent",
    buildPrompt: (ctx, curl) =>
      [
        `${REQUEST_FROM_CURL_PROMPT_PREFIX}`,
        "",
        curl?.trim() ?? "",
        "",
        `Целевой файл: \`${ctx.path}\`.`,
        "Прочитай текущий request.adoc и соседние файлы метода (method.adoc, response.adoc, *.puml), сохрани структуру шаблона и заполни секции параметров, пример запроса и возможные ошибки.",
      ].join("\n"),
  },
];

export function resolveEditorContextActions(ctx: EditorActionContext): EditorContextAction[] {
  return EDITOR_CONTEXT_ACTIONS.filter((action) => action.when(ctx));
}
