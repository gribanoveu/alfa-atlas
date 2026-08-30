import type { AiAccessMode, ConversationMode } from "./aiTools";
import { basename } from "./assistantConfig";
import { REQUEST_FROM_CURL_PROMPT_PREFIX, isMethodDescriptionFile } from "./editorContextActions";

// Suggestion chips shown in the assistant panel's empty-state placeholder
// (`AssistantConversation`) and, once a branch is picked, in the follow-up
// bar above the composer. Clicking one fills the compose box — via a small
// input form when the suggestion declares `inputs` — without sending, so the
// user can still read and edit before submitting.
//
// Conventions this list follows, and why:
//
// - `text` states the GOAL and the BOUNDARIES, never the tool route. A baked-in
//   route ("check → listFiles → semanticSearch → readFile") breaks the moment a
//   tool is renamed and stops the model from taking a shorter path; per-mode
//   passes belong in the mode's system prompt (`assistantConfig.ts`).
// - User input arrives only through `inputs` + `{{key}}` placeholders — never
//   by leaving the prompt dangling ("Название метода - ") and hoping the user
//   notices they're expected to finish the sentence.
// - `mode` / `access` / `writes` are declarative: the UI switches the
//   conversation mode, asks for the repo-access upgrade, and badges the chip
//   itself, instead of the prompt asking the model in prose to please do it.
// - `mode` also decides WHERE a suggestion is offered: the empty-state chip
//   row shows only the suggestions belonging to the currently selected
//   Агент/План/Вопрос mode, so picking a mode picks a set of tasks that mode
//   can actually carry out (Plan mode produces plans, not edits). Follow-ups
//   are deliberately exempt — see `suggestionsForMode`.
// - `appliesTo` hides a suggestion where it makes no sense (nothing open, no
//   uncommitted changes), so the panel stays a short relevant list rather than
//   a static menu people stop reading.
//
// `followUps` makes this a (recursive, arbitrarily-deep) tree rather than a
// flat list: once the user picks a branch and sends that first message,
// `AssistantConversation` shows that node's `followUps` (if any) as a new chip
// row above the transcript — picking one of those advances to *that* node, so a
// node with its own `followUps` chains further, and a leaf node simply makes
// the row disappear. `id` must stay unique and stable across the whole tree
// (React key, plus tracking which node is "active").

/** One value the user fills in before the prompt is built. `key` is what
 * `{{key}}` in `text` refers to, and is also the handle used to carry a value
 * into a follow-up (see `prefillValues`) so e.g. the method name is asked for
 * once, not at every step. */
export interface SuggestionInput {
  key: string;
  label: string;
  placeholder: string;
  /** Blocks the form's submit while empty. Defaults to false. */
  required?: boolean;
  /** Renders a textarea instead of a single-line input. */
  multiline?: boolean;
}

/** What the panel knows about the user's situation when it decides which
 * chips to show. Built by `buildSuggestionContext` from props
 * `AssistantConversation` already receives. */
export interface SuggestionContext {
  /** The chat's current Агент/План/Вопрос mode — decides which set of
   * suggestions the empty state offers. */
  conversationMode: ConversationMode;
  /** Docs-root-relative path of the open editor tab, or null. */
  activeFilePath: string | null;
  /** The open tab is a REST method description (`{folder}/{folder}.adoc`). */
  isMethodDoc: boolean;
  /** The repo has uncommitted changes to *tracked* files — the ones `gitDiff`
   * can actually show the assistant. */
  hasUncommittedChanges: boolean;
}

export interface AssistantSuggestion {
  id: string;
  /** Imperative and short — this is the chip's own text. */
  label: string;
  /** One line under the chip row: what happens after the click. */
  hint?: string;
  /** Prompt template; `{{key}}` placeholders are filled from `inputs`. */
  text: string;
  inputs?: SuggestionInput[];
  /** Conversation mode this task needs. Required, because it does double
   * duty: it decides which mode's chip row this suggestion appears in, and
   * a follow-up that needs a different mode than the one the user is in
   * switches to it (silently — the mode chips right above show the result). */
  mode: ConversationMode;
  /** Repo access this task needs. `"fullRepo"` is an escalation, so the UI
   * asks for it explicitly in the form instead of flipping it on a click. */
  access?: AiAccessMode;
  /** Whether the assistant will change files. Badged on the chip so a stray
   * click on a read-only suggestion is visibly harmless. Required, not
   * optional: forgetting it should be a type error, not a silent "false". */
  writes: boolean;
  /** Hidden entirely when this returns false. */
  appliesTo?: (ctx: SuggestionContext) => boolean;
  followUps?: AssistantSuggestion[];
}

/** `access` left unset means "documentation only" — the safe default, and the
 * app's own default access mode. */
export function suggestionAccess(suggestion: AssistantSuggestion): AiAccessMode {
  return suggestion.access ?? "docsOnly";
}

/** True when running this suggestion would widen the assistant's reach from
 * the docs subtree to the whole repository. Drives both the lock badge on the
 * chip and the consent row in the form. */
export function needsAccessUpgrade(
  suggestion: AssistantSuggestion,
  current: AiAccessMode,
): boolean {
  return suggestionAccess(suggestion) === "fullRepo" && current !== "fullRepo";
}

/** True when clicking the chip should open the input form rather than fill the
 * composer straight away — either there's something to type, or there's a
 * privilege escalation to confirm. */
export function needsSuggestionForm(
  suggestion: AssistantSuggestion,
  current: AiAccessMode,
): boolean {
  return Boolean(suggestion.inputs?.length) || needsAccessUpgrade(suggestion, current);
}

/** Substitutes `{{key}}` for the user's answers. Values are trimmed; an
 * omitted (necessarily optional) input collapses to an empty string rather
 * than leaving raw braces in the prompt. */
export function renderSuggestionText(
  suggestion: AssistantSuggestion,
  values: Record<string, string>,
): string {
  let text = suggestion.text;
  for (const input of suggestion.inputs ?? []) {
    const value = (values[input.key] ?? "").trim();
    text = text.split(`{{${input.key}}}`).join(value);
  }
  return text;
}

/** Seeds a form from values the user already gave earlier in the same branch,
 * matched by `key` — so «Описание запроса из curl» after «Документация на
 * новый метод» doesn't ask for the method name a second time. */
export function prefillValues(
  suggestion: AssistantSuggestion,
  remembered: Record<string, string>,
): Record<string, string> {
  const seeded: Record<string, string> = {};
  for (const input of suggestion.inputs ?? []) {
    const value = remembered[input.key];
    if (value) seeded[input.key] = value;
  }
  return seeded;
}

/** True when every `required` input has a non-blank value. */
export function suggestionFormComplete(
  suggestion: AssistantSuggestion,
  values: Record<string, string>,
): boolean {
  return (suggestion.inputs ?? []).every(
    (input) => !input.required || (values[input.key] ?? "").trim().length > 0,
  );
}

/** The chip row for a fresh chat: what this mode can actually do, minus
 * whatever doesn't apply right now.
 *
 * Only the root row is filtered by mode. A follow-up is the continuation of a
 * branch the user already started — «Закрыть один пробел» after «Найти
 * пробелы» has to stay reachable even though it needs Агент and the user is
 * still in Вопрос; clicking it switches the mode. Filtering those by the
 * current mode would hide exactly the next step the user just earned. */
export function suggestionsForMode(
  suggestions: AssistantSuggestion[],
  ctx: SuggestionContext,
): AssistantSuggestion[] {
  return visibleSuggestions(suggestions, ctx).filter((s) => s.mode === ctx.conversationMode);
}

export function visibleSuggestions(
  suggestions: AssistantSuggestion[],
  ctx: SuggestionContext,
): AssistantSuggestion[] {
  return suggestions.filter((s) => (s.appliesTo ? s.appliesTo(ctx) : true));
}

export function buildSuggestionContext(input: {
  conversationMode: ConversationMode;
  activeFilePath: string | null;
  hasUncommittedChanges: boolean;
}): SuggestionContext {
  const { conversationMode, activeFilePath, hasUncommittedChanges } = input;
  return {
    conversationMode,
    activeFilePath,
    isMethodDoc: activeFilePath
      ? isMethodDescriptionFile({ path: activeFilePath, basename: basename(activeFilePath) })
      : false,
    hasUncommittedChanges,
  };
}

export const ASSISTANT_SUGGESTIONS: AssistantSuggestion[] = [
  // ---- Агент: подсказки, которые действительно правят документацию ----

  {
    id: "new-method-doc",
    label: "Документация на новый метод",
    hint: "Создаёт папку с заготовками .adoc и диаграммой",
    writes: true,
    mode: "agent",
    access: "docsOnly",
    inputs: [
      {
        key: "method",
        label: "Название метода",
        placeholder: "createSignOperationV2",
        required: true,
      },
    ],
    text:
      "Заведи документацию на метод {{method}}: папку с этим названием в разделе документации " +
      "и внутри неё заготовки по стандартному шаблону REST-метода — {{method}}.adoc, request.adoc, " +
      "response.adoc и .puml с диаграммой последовательности.\n" +
      "Содержимое не заполняй: нужны только каркас, шапка и пустые секции с include на request/response " +
      "и на диаграмму. Если папка с таким названием уже есть — не перезаписывай её, сообщи и остановись.\n" +
      "В конце покажи дерево созданных файлов.",
    followUps: [
      {
        id: "new-method-doc.from-curl",
        label: "Описание запроса из curl",
        hint: "Заполняет таблицу входных параметров и пример запроса",
        writes: true,
        mode: "agent",
        access: "docsOnly",
        inputs: [
          {
            key: "curl",
            label: "curl-запрос",
            placeholder: "curl -X POST https://... -H 'A-userId: XAAAAA' ...",
            required: true,
            multiline: true,
          },
        ],
        text:
          `${REQUEST_FROM_CURL_PROMPT_PREFIX}\n\n` +
          "```\n{{curl}}\n```\n\n" +
          "Заполни request.adoc и добавь таблицу входных параметров в раздел «Входные параметры» " +
          "основного .adoc: метод, эндпоинт, заголовки, path- и body-параметры с форматом, " +
          "обязательностью и описанием.\n" +
          "Значения, которые выглядят как реальные данные клиента, замени на типовые примеры " +
          "(XAAAAA, UAAAAA); токены, ключи и секреты в документацию не переноси.\n" +
          "Чего в curl нет — обязательности, вариантов значений — не выдумывай: оставь пустым " +
          "и перечисли в конце, что нужно уточнить.",
      },
      {
        id: "new-method-doc.response-example",
        label: "Пример ответа",
        hint: "Успешный и ошибочный ответ в response.adoc",
        writes: true,
        mode: "agent",
        access: "docsOnly",
        text:
          "Добавь в response.adoc пример успешного ответа и пример ошибочного ответа. " +
          "Структуру бери из таблицы выходных параметров и из описания ошибок в основном .adoc — " +
          "коды, type и code должны совпадать с тем, что уже описано в документе, а не быть новыми. " +
          "Если таблица выходных параметров пустая, скажи об этом и не придумывай поля.",
      },
      {
        id: "new-method-doc.sequence-diagram",
        label: "Диаграмма последовательности",
        hint: "Собирает PlantUML по шагам алгоритма",
        writes: true,
        mode: "agent",
        access: "docsOnly",
        text:
          "Собери диаграмму последовательности в .puml по разделу «Алгоритм работы»: " +
          "участники — вызывающая система, наш сервис и все внешние сервисы из шагов; " +
          "стрелки подписаны номерами и названиями шагов; ветки ошибок показаны там, где шаг " +
          "может остановить работу метода.\n" +
          "Ничего, чего нет в алгоритме, на диаграмму не добавляй. " +
          "Если шаги описаны не полностью, сначала перечисли, чего не хватает.",
      },
    ],
  },

  {
    id: "update-section",
    label: "Обновить раздел",
    hint: "Переписывает раздел открытого файла под новое поведение",
    writes: true,
    mode: "agent",
    access: "docsOnly",
    appliesTo: (ctx) => Boolean(ctx.activeFilePath),
    inputs: [
      {
        key: "change",
        label: "Что изменилось",
        placeholder: "добавили заголовок A-projectId, он обязательный",
        required: true,
        multiline: true,
      },
    ],
    text:
      "Обнови раздел документации в открытом файле под изменение: {{change}}\n" +
      "Меняй минимально — только те строки и ячейки таблиц, которых касается изменение; " +
      "формулировки, порядок разделов и стиль соседнего текста сохраняй. " +
      "Проверь, не разъехались ли связанные места: таблицы входных/выходных параметров, " +
      "описание шага, сводная таблица ошибок, примеры запроса и ответа.\n" +
      "Сначала покажи, что собираешься поменять, и только потом правь файл. В конце — diff.",
  },

  {
    id: "describe-algorithm",
    label: "Описать алгоритм работы",
    hint: "Расписывает шаги метода по реализации в коде",
    writes: true,
    mode: "agent",
    access: "fullRepo",
    appliesTo: (ctx) => ctx.isMethodDoc,
    text:
      "Заполни раздел «Алгоритм работы» в открытом описании метода по тому, " +
      "как метод реализован в коде.\n" +
      "Шаги нумеруй по порядку выполнения; для каждого укажи, что происходит, " +
      "какой внешний сервис вызывается (метод и назначение) и при каком условии " +
      "работа прерывается с ошибкой. Валидации и проверки прав — тоже шаги.\n" +
      "Пиши по факту кода: если ветка есть в коде, но не описана — добавь; если " +
      "описана, но в коде её нет — не переноси, вынеси отдельным списком. " +
      "Ничего, чего нет в реализации, не придумывай. В конце покажи diff.",
  },

  {
    id: "describe-errors",
    label: "Описать ошибки метода",
    hint: "Собирает сводную таблицу ошибок по коду",
    writes: true,
    mode: "agent",
    access: "fullRepo",
    appliesTo: (ctx) => ctx.isMethodDoc,
    text:
      "Заполни сводную таблицу ошибок в открытом описании метода: пройди по коду " +
      "и собери все ошибки, которые метод может вернуть.\n" +
      "Для каждой — HTTP-код, type и code, текст сообщения и условие, при котором " +
      "она возникает, со ссылкой на место в коде. Коды и тексты бери из кода " +
      "дословно, не переформулируй и не придумывай отсутствующие.\n" +
      "Сверься с описанием шагов алгоритма: если шаг может завершиться ошибкой, " +
      "которой нет в таблице, добавь её; если в таблице есть ошибка, которую код " +
      "не возвращает, не удаляй молча — вынеси отдельным списком. В конце покажи diff.",
  },

  {
    id: "format-to-standard",
    label: "Оформить по стандарту",
    hint: "Приводит открытый файл к принятому оформлению",
    writes: true,
    mode: "agent",
    access: "docsOnly",
    appliesTo: (ctx) => Boolean(ctx.activeFilePath),
    inputs: [
      {
        key: "scope",
        label: "Что оформить",
        placeholder: "весь файл или название раздела",
        required: true,
      },
    ],
    text:
      "Приведи к принятому в проекте оформлению: {{scope}} — в открытом файле. " +
      "Ориентируйся на то, как оформлены соседние документы: уровни заголовков, " +
      "оформление таблиц, подписи и включения, единый стиль формулировок и терминов.\n" +
      "Смысл не меняй: это правка оформления, а не содержания. Если по ходу заметишь " +
      "фактическую ошибку — не исправляй её молча, вынеси отдельным списком.\n" +
      "В конце покажи diff.",
  },

  // ---- План: только исследование и `createPlan`, выполняет потом Агент ----

  {
    id: "plan-jira-task",
    label: "План по задаче",
    hint: "Раскладывает постановку на шаги по документации",
    writes: false,
    mode: "plan",
    access: "docsOnly",
    inputs: [
      {
        key: "task",
        label: "Задача",
        placeholder: "номер задачи и постановка — можно скопировать целиком",
        required: true,
        multiline: true,
      },
    ],
    text:
      "Разложи задачу на шаги по документации:\n\n{{task}}\n\n" +
      "Сначала найди, что по этой теме уже написано, и только потом планируй. " +
      "Каждый шаг — один файл или один раздел, с путём и с тем, что в нём сделать.\n" +
      "Отдельно перечисли, чего в постановке не хватает, чтобы работу можно было " +
      "довести до конца без догадок. Ничего не правь.",
  },

  {
    id: "plan-feature-docs",
    label: "План документирования фичи",
    hint: "Изучит код и составит план: что завести и что обновить",
    writes: false,
    mode: "plan",
    access: "fullRepo",
    inputs: [
      {
        key: "feature",
        label: "Фича",
        placeholder: "отправка документов на подпись",
        required: true,
      },
    ],
    text:
      "Составь план документирования фичи: {{feature}}. Сначала разберись по коду, " +
      "как она устроена, и посмотри, что о ней уже написано.\n" +
      "В плане: какие документы завести и какие обновить (с путями), в каком порядке, " +
      "и что нужно уточнить у команды до начала работы.\n" +
      "Шаги должны быть выполнимыми по одному. Ничего не меняй — только план.",
  },

  {
    id: "plan-api-change",
    label: "План правок под изменение API",
    hint: "Оценит, какие разделы задеты, и разложит работу по шагам",
    writes: false,
    mode: "plan",
    access: "docsOnly",
    inputs: [
      {
        key: "change",
        label: "Что изменилось в API",
        placeholder: "добавили заголовок A-projectId, он обязательный",
        required: true,
        multiline: true,
      },
    ],
    text:
      "Составь план правок документации под изменение: {{change}}\n" +
      "Пройди по документации и найди все места, которых это касается: таблицы " +
      "параметров, описания шагов, сводные таблицы ошибок, примеры запроса и ответа, " +
      "диаграммы.\n" +
      "В плане каждый шаг — один файл или один раздел, с путём и с тем, что именно " +
      "в нём поменять. Ничего не правь.",
  },

  {
    id: "plan-cleanup",
    label: "План приведения раздела в порядок",
    hint: "Обходит раздел и предлагает порядок работ",
    writes: false,
    mode: "plan",
    access: "docsOnly",
    inputs: [
      {
        key: "scope",
        label: "Раздел",
        placeholder: "путь к папке или название раздела",
        required: true,
      },
    ],
    text:
      "Обойди раздел {{scope}} и составь план, как привести его в порядок: " +
      "пустые и недописанные места, расхождения между соседними документами, " +
      "нарушения принятого оформления, битые include и ссылки.\n" +
      "Сортируй шаги по пользе для читателя, а не по порядку обхода, и не " +
      "раздувай план: несколько однотипных мелких правок — один шаг. Ничего не правь.",
  },

  // ---- Вопрос: разбор и ревью, без единой правки ----

  {
    id: "find-gaps",
    label: "Найти пробелы",
    hint: "Только чтение: список мест, где не хватает описания",
    writes: false,
    mode: "question",
    access: "docsOnly",
    text:
      "Найди в документации проекта места, где описание отсутствует или недоделано. " +
      "Считай проблемой пустые секции, заголовки-заглушки, TODO-комментарии, битые include " +
      "и REST-методы без request.adoc или response.adoc.\n" +
      "Верни 3–5 самых существенных мест: путь, что именно не хватает, почему это мешает читателю. " +
      "Сортируй по важности, а не по порядку обхода. Ничего не правь и не создавай.",
    followUps: [
      {
        id: "find-gaps.fix-one",
        label: "Закрыть один пробел",
        hint: "Правит только выбранный файл",
        writes: true,
        mode: "agent",
        access: "docsOnly",
        inputs: [
          {
            key: "target",
            label: "Какой пробел закрыть",
            placeholder: "1 или путь к файлу",
            required: true,
          },
        ],
        text:
          "Закрой пробел: {{target}}. Правь только этот файл. " +
          "Пиши по принятому стандарту оформления и не трогай соседние разделы. " +
          "В конце покажи diff.",
      },
    ],
  },

  {
    id: "sync-with-code",
    label: "Сверить с кодом",
    hint: "Только чтение: список расхождений документации и реализации",
    writes: false,
    mode: "question",
    access: "fullRepo",
    inputs: [
      {
        key: "scope",
        label: "Что сверяем",
        placeholder: "метод, раздел или путь к файлу документации",
        required: true,
      },
    ],
    text:
      "Сверь документацию и код по: {{scope}}. Источник истины — код.\n" +
      "Проверь состав и обязательность параметров, коды и тексты ошибок, порядок вызовов " +
      "внешних сервисов, формат ответа.\n" +
      "Верни таблицу расхождений: что написано в документации, что на самом деле в коде, " +
      "ссылка на файл и место в коде. Ничего не правь — что чинить, решу я.",
  },

  {
    id: "explain-feature",
    label: "Объяснить фичу",
    hint: "Разбор по коду, со ссылками на файлы",
    writes: false,
    mode: "question",
    access: "fullRepo",
    inputs: [
      {
        key: "feature",
        label: "Фича",
        placeholder: "отправка документов на подпись",
        required: true,
      },
    ],
    text:
      "Объясни, как работает фича: {{feature}}. Главный источник истины — код: " +
      "разбирай реализацию по файлам и сигнатурам, а не по названиям и структуре папок. " +
      "Документацию используй только для сверки терминов.\n" +
      "В ответе: поток выполнения по шагам со ссылками на конкретные файлы и функции, " +
      "затем расхождения с документацией (показывай оба варианта и помечай фактический), " +
      "затем отдельным списком — что осталось предположением.\n" +
      "Поведения, которого нет в коде, не придумывай.",
    followUps: [
      {
        id: "explain-feature.document-it",
        label: "Описать это в документации",
        hint: "Переносит разобранное поведение в .adoc",
        writes: true,
        mode: "agent",
        access: "docsOnly",
        text:
          "Опиши разобранное поведение в документации: найди подходящий раздел, а если его нет — " +
          "скажи, где его завести, и дождись ответа. Пиши только то, что подтверждено кодом выше; " +
          "предположения из разбора в документацию не переноси.\n" +
          "Соблюдай принятый стандарт оформления и не трогай соседние разделы. В конце покажи diff.",
      },
    ],
  },

  {
    id: "review-doc-changes",
    label: "Проверить мои правки",
    hint: "Ревью незакоммиченных изменений в документации",
    writes: false,
    mode: "question",
    access: "docsOnly",
    appliesTo: (ctx) => ctx.hasUncommittedChanges,
    text:
      "Посмотри мои незакоммиченные изменения в документации и сделай ревью: " +
      "нарушения принятого стиля оформления, рассинхрон между таблицами и текстом, " +
      "битые include и ссылки, ошибки, описанные в шаге, но не попавшие в сводную таблицу.\n" +
      "Верни замечания списком, у каждого — файл, строка и предлагаемая формулировка. Сам не правь.",
  },
];
