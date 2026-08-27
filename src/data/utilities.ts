import { Clock, Hash, KeyRound, type LucideIcon } from "lucide-react";

export type UtilityId = "unixtime" | "ids" | "jwt";

export type UtilityDef = {
  id: UtilityId;
  title: string;
  description: string;
  icon: LucideIcon;
  /** Утилита пока без реализации — вкладка открывается как заглушка. */
  stub: boolean;
};

/**
 * Утилиты, показанные карточками в панели «Утилиты». Клик по карточке
 * открывает псевдо-вкладку в редакторе (см. `useEditorTabActions`) —
 * файла за ней нет, поэтому она живёт вне `useEditorTabs`, как API Explorer.
 */
export const UTILITIES: UtilityDef[] = [
  {
    id: "unixtime",
    title: "Конвертер Unixtime",
    description: "Перевод Unix-времени в дату и обратно",
    icon: Clock,
    stub: false,
  },
  {
    id: "ids",
    title: "Генератор UUID и ULID",
    description: "Случайные UUID v4 и сортируемые ULID",
    icon: Hash,
    stub: false,
  },
  {
    id: "jwt",
    title: "Парсер JWT",
    description: "Разбор заголовка и payload токена",
    icon: KeyRound,
    stub: false,
  },
];

const TAB_ID_PREFIX = "utility:";

export function utilityTabId(id: UtilityId): string {
  return `${TAB_ID_PREFIX}${id}`;
}

export function utilityIdFromTabId(tabId: string): UtilityId | null {
  if (!tabId.startsWith(TAB_ID_PREFIX)) return null;
  const id = tabId.slice(TAB_ID_PREFIX.length);
  return UTILITIES.some((utility) => utility.id === id) ? (id as UtilityId) : null;
}

export function findUtility(id: UtilityId): UtilityDef | undefined {
  return UTILITIES.find((utility) => utility.id === id);
}
