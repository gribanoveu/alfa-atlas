import { useEffect, useMemo, useRef, useState, type ComponentType } from "react";
import {
  Bell,
  ClipboardCheck,
  Cpu,
  Database,
  FileCode,
  FolderTree,
  KeyRound,
  Palette,
  Puzzle,
  ScrollText,
  Search,
  Settings2,
  ShieldCheck,
  Sparkles,
  SpellCheck,
  Ticket,
  X,
} from "lucide-react";
import type { GeneralPrefs } from "../../lib/prefs";
import type { SpellcheckConfig } from "../../lib/spellcheck";
import { useGeneralPrefsEditor } from "../../hooks/useGeneralPrefsEditor";
import "../Welcome/CloneRepoModal.css";
import "./SettingsDialog.css";
import { AppearanceTab } from "./AppearanceTab";
import { AssistantBehaviorTab } from "./AssistantBehaviorTab";
import { CredentialsTab } from "./CredentialsTab";
import { EditorTab } from "./EditorTab";
import { EmbeddingsTab } from "./EmbeddingsTab";
import { GeneralTab } from "./GeneralTab";
import { JiraTab } from "./JiraTab";
import { LlmTab } from "./LlmTab";
import { LoggingTab } from "./LoggingTab";
import { NotificationsTab } from "./NotificationsTab";
import { PermissionsTab } from "./PermissionsTab";
import { SkillsTab } from "./SkillsTab";
import { SpellcheckTab } from "./SpellcheckTab";
import { StandardsRulesTab } from "./StandardsRulesTab";
import { WorkspaceTab } from "./WorkspaceTab";

export type SectionId =
  | "general"
  | "appearance"
  | "workspace"
  | "editor"
  | "spellcheck"
  | "standards"
  | "llm"
  | "assistant"
  | "skills"
  | "permissions"
  | "notifications"
  | "jira"
  | "credentials"
  | "embeddings"
  | "logging";

type Section = {
  id: SectionId;
  /** Short label in the sidebar. */
  label: string;
  /** Heading above the section's content — may be longer than `label`. */
  title: string;
  /** One line under the heading; every section carries its own, so the tab
   * components no longer repeat a lead paragraph of their own. */
  description: string;
  icon: ComponentType<{ size?: number; className?: string; "aria-hidden"?: boolean }>;
  /** Extra terms the sidebar filter matches on, beyond label and title. */
  keywords: string;
};

type SectionGroup = { label: string; sections: Section[] };

/** Sections grouped by what the user is actually configuring: the app
 * itself, the editing surface, the assistant, and the outside world it
 * talks to. The order inside each group runs from the most commonly
 * changed setting to the most niche. */
const GROUPS: SectionGroup[] = [
  {
    label: "Приложение",
    sections: [
      {
        id: "general",
        label: "Общие",
        title: "Общие",
        description:
          "Поведение при запуске и общие настройки приложения",
        icon: Settings2,
        keywords: "запуск проект welcome язык ошибки диагностики",
      },
      {
        id: "appearance",
        label: "Внешний вид",
        title: "Внешний вид",
        description:
          "Размер шрифта по зонам интерфейса и прочие настройки внешнего вида",
        icon: Palette,
        keywords: "шрифт размер интерфейс редактор превью сайдбар external дерево",
      },
      {
        id: "workspace",
        label: "Файлы и папки",
        title: "Файлы и папки",
        description:
          "Расположение файлов и папок, а также подерживаемые форматы файлов",
        icon: FolderTree,
        keywords: "пути папка настроек atlas планы форматы расширения",
      },
    ],
  },
  {
    label: "Редактор",
    sections: [
      {
        id: "editor",
        label: "Редактирование",
        title: "Редактирование",
        description:
          "Когда изменения попадают на диск и как разрешаются ссылки в OpenAPI-спеках",
        icon: FileCode,
        keywords: "автосохранение задержка вкладки openapi ref common спек",
      },
      {
        id: "spellcheck",
        label: "Орфография",
        title: "Проверка орфографии",
        description:
          "Подсвечивает слова с ошибкой прямо в редакторе. Слово считается ошибкой, только если его нет ни в одном включённом словаре",
        icon: SpellCheck,
        keywords: "словарь camelCase txt личный словарь опечатки",
      },
      {
        id: "standards",
        label: "Стандарты",
        title: "Стандарты API-документации",
        description:
          "Правила проверки документации методов API на соответствие корпоративному стандарту. Выключенное правило не участвует в подсчёте баллов",
        icon: ClipboardCheck,
        keywords: "правила проверка баллы вес api документация",
      },
    ],
  },
  {
    label: "Ассистент",
    sections: [
      {
        id: "llm",
        label: "Провайдеры",
        title: "Провайдеры LLM",
        description:
          "Языковая модель для чата с ассистентом. Встроенные провайдеры уже настроены — нужен только API-ключ; можно дополнительно добавить свой провайдер LLM",
        icon: Cpu,
        keywords: "llm api ключ base url модель сертификат лимиты токены",
      },
      {
        id: "assistant",
        label: "Поведение",
        title: "Поведение ассистента",
        description:
          "Долгосрочная память и подсказки",
        icon: Sparkles,
        keywords: "память факты подсказки follow-up предложения чат",
      },
      {
        id: "skills",
        label: "Скилы",
        title: "Скилы ассистента",
        description:
          "Специализированные инструкции в формате Agent Skills. Ассистент ищет их через тул skill — полный список в промпт не попадает, а выключенный скил не находится поиском",
        icon: Puzzle,
        keywords: "agent skills инструкции встроенные пользовательские",
      },
      {
        id: "permissions",
        label: "Разрешения",
        title: "Разрешения инструментов",
        description:
          "Что ассистент может делать в текущем проекте и какие действия выполняются без подтверждения",
        icon: ShieldCheck,
        keywords: "инструменты доступ автоодобрение разрешать всегда отозвать",
      },
      {
        id: "notifications",
        label: "Уведомления",
        title: "Уведомления ассистента",
        description:
          "Звук и системные уведомления о ходе работы",
        icon: Bell,
        keywords: "звук баннер завершение вопрос",
      },
    ],
  },
  {
    label: "Интеграции",
    sections: [
      {
        id: "jira",
        label: "Jira",
        title: "Jira",
        description:
          "Адрес экземпляра Jira и токен, которым приложение к нему подключается",
        icon: Ticket,
        keywords: "jira задачи тикеты токен pat personal access token сертификат инстанс",
      },
    ],
  },
  {
    label: "Данные и доступ",
    sections: [
      {
        id: "credentials",
        label: "Git",
        title: "Доступ к Git",
        description:
          "SSH-ключ, которым приложение авторизуется в репозиториях",
        icon: KeyRound,
        keywords: "ssh ключ ed25519 known_hosts репозиторий авторизация",
      },
      {
        id: "embeddings",
        label: "Поиск и индекс",
        title: "Эмбеддинги и индекс",
        description:
          "Семантический индекс чанков документации: где считаются векторы и в каком состоянии индекс текущего проекта",
        icon: Database,
        keywords: "эмбеддинги вектор bge локально api индекс синхронизация поиск",
      },
      {
        id: "logging",
        label: "Диагностика",
        title: "Журналы",
        description:
          "Журналы запросов к модели и вызовов инструментов. Полезны при разборе ошибок, но могут содержать чувствительные данные. Включенное логгирование запросов и ответов модели на постоянной основе может занимать много места на диске, используйте его только при необходимости",
        icon: ScrollText,
        keywords: "логи logging jsonl tool calls отладка",
      },
    ],
  },
];

const ALL_SECTIONS: Section[] = GROUPS.flatMap((group) => group.sections);

function tokenize(text: string): string[] {
  return text.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean);
}

/** Word-prefix match rather than a raw substring: Russian settings copy is
 * full of words like «выключено», which a substring search would hand back
 * for the query «ключ». Every query word has to prefix some word of the
 * section, so «шриф» still finds «Размер шрифта». */
function matches(section: Section, queryWords: string[]): boolean {
  const words = tokenize(
    `${section.label} ${section.title} ${section.description} ${section.keywords}`,
  );
  return queryWords.every((needle) => words.some((word) => word.startsWith(needle)));
}

type SettingsDialogProps = {
  projectRoot: string | null;
  onClose: () => void;
  onPrefsChange?: (prefs: GeneralPrefs) => void;
  onSpellcheckConfigChange?: (config: SpellcheckConfig) => void;
  initialSection?: SectionId;
};

export function SettingsDialog({
  projectRoot,
  onClose,
  onPrefsChange,
  onSpellcheckConfigChange,
  initialSection,
}: SettingsDialogProps) {
  const prefsEditor = useGeneralPrefsEditor(onPrefsChange);
  const [section, setSection] = useState<SectionId>(initialSection ?? "general");
  const [query, setQuery] = useState("");
  const contentRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (initialSection) setSection(initialSection);
  }, [initialSection]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  // Each section starts at the top rather than inheriting the previous
  // section's scroll offset.
  useEffect(() => {
    contentRef.current?.scrollTo({ top: 0 });
  }, [section]);

  const filteredGroups = useMemo(() => {
    const queryWords = tokenize(query);
    if (queryWords.length === 0) return GROUPS;
    return GROUPS.map((group) => ({
      ...group,
      sections: group.sections.filter((item) => matches(item, queryWords)),
    })).filter((group) => group.sections.length > 0);
  }, [query]);

  const visibleSections = useMemo(
    () => filteredGroups.flatMap((group) => group.sections),
    [filteredGroups],
  );

  const firstMatch = visibleSections[0] ?? null;
  const searchActive = tokenize(query).length > 0;
  const noSearchResults = searchActive && visibleSections.length === 0;

  useEffect(() => {
    if (!searchActive) return;
    if (visibleSections.length === 0) return;
    if (visibleSections.some((item) => item.id === section)) return;
    setSection(visibleSections[0]!.id);
  }, [searchActive, visibleSections, section]);

  const active =
    ALL_SECTIONS.find((item) => item.id === section) ?? ALL_SECTIONS[0]!;
  const ActiveIcon = active.icon;

  return (
    <div className="settings-backdrop" role="presentation" onClick={onClose}>
      <div
        className="settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-dialog-title"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="settings-header">
          <h2 className="settings-title" id="settings-dialog-title">
            Настройки
          </h2>
          <button
            type="button"
            className="settings-close"
            onClick={onClose}
            aria-label="Закрыть"
          >
            <X size={15} aria-hidden />
          </button>
        </header>

        <div className="settings-body">
          <nav className="settings-nav" aria-label="Разделы настроек">
            <div className="settings-search">
              <Search size={13} className="settings-search-icon" aria-hidden />
              <input
                type="text"
                className="settings-search-input"
                placeholder="Поиск настроек"
                aria-label="Поиск по разделам настроек"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && firstMatch) {
                    setSection(firstMatch.id);
                  }
                  if (event.key === "Escape" && query) {
                    event.stopPropagation();
                    setQuery("");
                  }
                }}
              />
              {query ? (
                <button
                  type="button"
                  className="settings-search-clear"
                  aria-label="Очистить поиск"
                  onClick={() => setQuery("")}
                >
                  <X size={12} aria-hidden />
                </button>
              ) : null}
            </div>

            <div className="settings-nav-list">
              {filteredGroups.map((group) => (
                <div key={group.label} className="settings-nav-group">
                  <div className="settings-nav-group-label">{group.label}</div>
                  {group.sections.map((item) => {
                    const Icon = item.icon;
                    const isActive = section === item.id;
                    return (
                      <button
                        key={item.id}
                        type="button"
                        className={`settings-nav-btn${isActive ? " active" : ""}`}
                        aria-current={isActive ? "page" : undefined}
                        onClick={() => setSection(item.id)}
                      >
                        <Icon size={14} className="settings-nav-icon" aria-hidden />
                        <span className="settings-nav-label">{item.label}</span>
                      </button>
                    );
                  })}
                </div>
              ))}
              {filteredGroups.length === 0 ? (
                <p className="settings-nav-empty">Ничего не найдено</p>
              ) : null}
            </div>
          </nav>

          <div className="settings-content" ref={contentRef}>
            <div className="settings-page">
              {noSearchResults ? (
                <p className="settings-content-empty">
                  Ничего не найдено по запросу «{query}»
                </p>
              ) : (
                <>
                  <header className="settings-page-head">
                    <div className="settings-page-title">
                      <ActiveIcon size={16} className="settings-page-icon" aria-hidden />
                      <h3>{active.title}</h3>
                    </div>
                    <p className="settings-page-desc">{active.description}</p>
                  </header>

                  {section === "general" ? <GeneralTab editor={prefsEditor} /> : null}
                  {section === "appearance" ? <AppearanceTab editor={prefsEditor} /> : null}
                  {section === "workspace" ? (
                    <WorkspaceTab projectRoot={projectRoot} editor={prefsEditor} />
                  ) : null}
                  {section === "editor" ? <EditorTab editor={prefsEditor} /> : null}
                  {section === "spellcheck" ? (
                    <SpellcheckTab onConfigChange={onSpellcheckConfigChange} />
                  ) : null}
                  {section === "standards" ? <StandardsRulesTab /> : null}
                  {section === "llm" ? <LlmTab /> : null}
                  {section === "assistant" ? <AssistantBehaviorTab /> : null}
                  {section === "skills" ? <SkillsTab /> : null}
                  {section === "permissions" ? <PermissionsTab /> : null}
                  {section === "notifications" ? <NotificationsTab /> : null}
                  {section === "jira" ? <JiraTab /> : null}
                  {section === "credentials" ? <CredentialsTab /> : null}
                  {section === "embeddings" ? <EmbeddingsTab repoRoot={projectRoot} /> : null}
                  {section === "logging" ? <LoggingTab /> : null}
                </>
              )}
            </div>
          </div>
        </div>

        <footer className="settings-footer">
          <span className="settings-footer-note">
            Изменения сохраняются сразу
          </span>
          <button type="button" className="settings-btn" onClick={onClose}>
            Закрыть
          </button>
        </footer>
      </div>
    </div>
  );
}
