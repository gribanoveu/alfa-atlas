import { FileCheck, FilePlus, GitBranch, Keyboard, KeyRound, type LucideIcon } from "lucide-react";
import addSshKeyToGitVideo from "../assets/onboarding/add_ssh_key_to_git.mov";
import cloneRepoVideo from "../assets/onboarding/clone-repo.mov";
import copySshKeyVideo from "../assets/onboarding/copy_ssh_key.mov";
import checkStandardsVideo from "../assets/onboarding/check_standarts.mov";
import createMethodFromTemplateVideo from "../assets/onboarding/create_method_from_template.mov";
import useShortcutsVideo from "../assets/onboarding/use_shortcut.mov";
import workWithGitVideo from "../assets/onboarding/commit_changes.mov";
import createBranchVideo from "../assets/onboarding/create_branch.mov";

export type OnboardingStep = {
  videoSrc: string;
  text: string;
};

export type OnboardingCardDef = {
  id: string;
  title: string;
  description: string;
  icon: LucideIcon;
  steps: OnboardingStep[];
};

/**
 * Onboarding cards shown in the "Начать работу" section of the
 * Notifications panel. Add or remove entries here — completion state
 * is tracked separately by id in ~/.atlas/onboarding.json, so removing
 * a card here simply drops it from the list.
 */
export const ONBOARDING_CARDS: OnboardingCardDef[] = [
  {
    id: "setup-git-ssh",
    title: "Настройте работу с Git",
    description: "Сгенерируйте SSH-ключ и подключите его к Bitbucket, чтобы синхронизировать репозитории.",
    icon: KeyRound,
    steps: [
      {
        videoSrc: copySshKeyVideo,
        text: "Приложение уже сгенерировало SSH-ключ для безопасного подключения к Git. Скопируйте открытый ключ — приватная часть остаётся только на этом компьютере и никуда не передаётся.",
      },
      {
        videoSrc: addSshKeyToGitVideo,
        text: "Добавьте ваш открытый ключ SSH в ваш Bitbucket для начала синхронизации с Git.",
      },
    ],
  },
  {
    id: "clone-repo",
    title: "Клонируйте репозиторий",
    description: "Узнайте, как клонировать существующий репозиторий прямо в приложении.",
    icon: GitBranch,
    steps: [
      {
        videoSrc: cloneRepoVideo,
        text: "Откройте окно клонирования и вставьте ссылку на репозиторий — приложение скачает его и откроет как проект.",
      },
    ],
  },
  {
    id: "work-with-git",
    title: "Работайте с Git",
    description: "Как отправить изменения в гит репозиторий.",
    icon: GitBranch,
    steps: [
      {
        videoSrc: createBranchVideo,
        text: "Создайте новую ветку для ваших изменений. Откройте панель Git, введите название ветки и нажмите на кнопку создать ветку. По нажатию на созданную ветку или любую другую происходит переключение на нее.",
      },
      {
        videoSrc: workWithGitVideo,
        text: "В секции Изменения по нажатию на файл можно увидеть какие изменения были внесены в файл. Для того чтобы файлы были добавлены в коммит, нажмите на плюс рядом с именем файла. Для того чтобы отправить изменения на сервер нажмите на панели кнопку для публикации или откройте меню Git -> Отправить изменения.",
      },
    ],
  },
  {
    id: "check-standards",
    title: "Проверьте стандарты документации",
    description: "Узнайте, как проверить стандарты документации в приложении.",
    icon: FileCheck,
    steps: [
      {
        videoSrc: checkStandardsVideo,
        text: "Откройте панель стандартов и и нажмите на кнопку проверки стандартов, приложение проверит все файлы в проекте и отобразит отчет.",
      },
    ],
  },
  {
    id: "create-method-from-template",
    title: "Создайте документацию из шаблона",
    description: "Узнайте, как создать документацию REST вызова из шаблона в приложении.",
    icon: FilePlus,
    steps: [
      {
        videoSrc: createMethodFromTemplateVideo,
        text: "Нажмите правой кнопкой в проводнике документации и выберите создать папку, укажите при создании использование шаблона. Приложение создаст папку со всеми необходимыми файлами для описания документации.",
      },
    ],
  },
  {
    id: "use-shortcuts",
    title: "Используйте горячие клавиши",
    description: "Узнайте, как использовать горячие клавиши в приложении.",
    icon: Keyboard,
    steps: [
      {
        videoSrc: useShortcutsVideo,
        text: "Нажмите в редакторе клавишу восклицательного знака, чтобы отобразить список шаблонных конструкций asciidoc. После вставки используйте клавишу Tab для перемещения между полями.",
      },
    ],
  },
];
