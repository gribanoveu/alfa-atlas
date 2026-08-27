import type { ConversationMode } from "./aiTools";
import { basename } from "./assistantConfig";
import { dirnameOf } from "./paths";
import type { EditorTabKind, EditorTabOrigin } from "../hooks/useEditorTabs";

/** How a context action delivers its prompt to the assistant panel. */
export type EditorContextActionDelivery = "send" | "draft";

/** Prompt prefix for filling request.adoc from a curl example. */
export const REQUEST_FROM_CURL_PROMPT_PREFIX =
  "Сформируй описание входящего запроса в request.adoc на основе следующего curl-запроса:";

/** Prompt for checking a REST method description against corporate API standards. */
export const METHOD_STANDARDS_CHECK_PROMPT_PREFIX =
  "Проверь описание метода на соответствие корпоративному стандарту документации API.";

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
  /** `send` posts immediately; `draft` fills the composer for the user to review. */
  delivery?: EditorContextActionDelivery;
  conversationMode?: ConversationMode;
  buildPrompt: (ctx: EditorActionContext, inputValue?: string) => string;
};

export function adocStem(name: string): string | null {
  if (!name.toLowerCase().endsWith(".adoc")) return null;
  const stem = name.slice(0, -".adoc".length);
  return stem.length > 0 ? stem : null;
}

export function parentFolderName(path: string): string | null {
  const dir = dirnameOf(path);
  if (dir === ".") return null;
  const segments = dir.split("/").filter(Boolean);
  return segments.length > 0 ? (segments[segments.length - 1] ?? null) : null;
}

/** True when the file is `{folderName}/{folderName}.adoc` (REST method doc). */
export function isMethodDescriptionFile(ctx: EditorActionContext): boolean {
  const stem = adocStem(ctx.basename);
  const folder = parentFolderName(ctx.path);
  if (!stem || !folder) return false;
  if (matchesBasename(ctx.basename, "request.adoc") || matchesBasename(ctx.basename, "response.adoc")) {
    return false;
  }
  return stem.localeCompare(folder, undefined, { sensitivity: "accent" }) === 0;
}

export function methodFolderPath(ctx: EditorActionContext): string {
  const dir = dirnameOf(ctx.path);
  return dir === "." ? ctx.path : dir;
}

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

function buildRequestFromCurlPrompt(ctx: EditorActionContext, curl: string | undefined): string {
  return [
    REQUEST_FROM_CURL_PROMPT_PREFIX,
    "",
    curl?.trim() ?? "",
    "",
    `Целевой файл: \`${ctx.path}\`.`,
    "Прочитай текущий request.adoc и соседние файлы метода (method.adoc, response.adoc, *.puml), сохрани структуру шаблона и заполни секции параметров, пример запроса и возможные ошибки.",
  ].join("\n");
}

function buildMethodStandardsCheckPrompt(ctx: EditorActionContext): string {
  const methodFolder = methodFolderPath(ctx);
  return [
    METHOD_STANDARDS_CHECK_PROMPT_PREFIX,
    "",
    `Файл описания метода: \`${ctx.path}\`.`,
    `Папка метода: \`${methodFolder}\`.`,
    "",
    `Вызови check с kind: "standards" для папки \`${methodFolder}\`. Прочитай \`${ctx.path}\` и при необходимости соседние request.adoc и response.adoc.`,
    "",
    "В ответе перечисли каждое нарушение: код критерия (К.x.x), что не так и как должно быть. Если нарушения есть — определи это Thrift (в url есть /tapi или в теле запроса есть userData) или REST метод и исправь их в файлах папки метода, используя структуру шаблона, вызови скилл method-spec если нужно уточнить формат документа и правила написания документации. После правок снова вызови check с kind: \"standards\" для папки метода.",
  ].join("\n");
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
    delivery: "send",
    conversationMode: "agent",
    buildPrompt: (ctx, curl) => buildRequestFromCurlPrompt(ctx, curl),
  },
  {
    id: "method-standards-check",
    label: "Исправить нарушения техстандарта",
    when: (ctx) =>
      ctx.llmReady &&
      ctx.tabKind === "text" &&
      ctx.tabOrigin === "project" &&
      isMethodDescriptionFile(ctx),
    input: { kind: "none" },
    delivery: "draft",
    conversationMode: "agent",
    buildPrompt: (ctx) => buildMethodStandardsCheckPrompt(ctx),
  },
];

export function resolveEditorContextActions(ctx: EditorActionContext): EditorContextAction[] {
  return EDITOR_CONTEXT_ACTIONS.filter((action) => action.when(ctx));
}
