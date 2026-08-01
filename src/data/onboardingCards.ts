import { GitBranch, KeyRound, type LucideIcon } from "lucide-react";
import addSshKeyToGitVideo from "../assets/onboarding/add_ssh_key_to_git.mov";
import cloneRepoVideo from "../assets/onboarding/clone-repo.mov";
import copySshKeyVideo from "../assets/onboarding/copy_ssh_key.mov";

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
    description:
      "Сгенерируйте SSH-ключ и подключите его к Bitbucket, чтобы синхронизировать репозитории.",
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
    title: "Клонирование репозитория",
    description:
      "Узнайте, как склонировать существующий репозиторий прямо в приложении.",
    icon: GitBranch,
    steps: [
      {
        videoSrc: cloneRepoVideo,
        text: "Откройте окно клонирования и вставьте ссылку на репозиторий — приложение скачает его и откроет как проект.",
      },
    ],
  },
];
