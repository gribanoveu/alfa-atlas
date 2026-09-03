import type { SourceRef } from "../../lib/openapi";

/** Экранирование сегмента по RFC 6901 (`~` → `~0`, `/` → `~1`). */
export function pointerSegment(segment: string): string {
  return segment.replace(/~/g, "~0").replace(/\//g, "~1");
}

export function operationPointer(path: string, method: string): string {
  return `/paths/${pointerSegment(path)}/${method}`;
}

/** Записи, отсортированные от самой длинной к самой короткой, — чтобы поиск
 * источника был обычным «первое совпадение по префиксу». */
export function buildSourceIndex(sources: SourceRef[]): SourceRef[] {
  return [...sources].sort((a, b) => b.pointer.length - a.pointer.length);
}

function isAncestorPointer(candidate: string, pointer: string): boolean {
  if (candidate === "") return true;
  return pointer === candidate || pointer.startsWith(`${candidate}/`);
}

/** Файл, из которого пришёл узел `pointer`. Точное совпадение выигрывает у
 * предка, предок — у корня: узел, объявленный прямо во входном документе,
 * находит корневую запись, а вложенный `$ref` — свою собственную. */
export function sourceForPointer(
  index: SourceRef[],
  pointer: string,
): SourceRef | null {
  return index.find((entry) => isAncestorPointer(entry.pointer, pointer)) ?? null;
}

/**
 * Строки, по которым в файле-источнике ищется нужное место. Порядок — от
 * самой точной к самой грубой: у ссылки на файл целиком (`operations/x.yaml`)
 * якорем служит `operationId`, у ссылки с фрагментом (`schemas/all.yaml#/Pet`)
 * — последний сегмент фрагмента, а если операция объявлена прямо во входном
 * документе — сам путь ручки.
 */
export function searchKeysForSource(
  source: SourceRef | null,
  operation: { operationId?: string; path?: string; method?: string },
): string[] {
  const keys: string[] = [];
  if (source && source.fragment) {
    const last = source.fragment.split("/").filter(Boolean).pop();
    if (last) keys.push(last.replace(/~1/g, "/").replace(/~0/g, "~"));
  }
  if (operation.operationId) keys.push(operation.operationId);
  if (operation.path) keys.push(operation.path);
  return keys;
}

/**
 * Номер строки (1-based), с которой начинается искомый узел. Сначала ищем
 * ключ YAML-отображения (`Pet:`, `listPets:`), затем — любое вхождение
 * строки: `operationId: listPets` устроит так же хорошо, как ключ.
 *
 * Точных позиций у нас нет: резолвер работает через `serde_yaml`, который
 * не хранит номера строк, а тащить ради этого парсер со спанами — заметно
 * дороже, чем текстовый поиск по файлу, который всё равно уже открыт.
 * Не нашли — возвращаем 1, файл просто откроется с начала.
 */
export function findSpecLine(text: string, keys: string[]): number {
  if (keys.length === 0) return 1;
  const lines = text.split(/\r?\n/);
  for (const key of keys) {
    const asMapKey = new RegExp(
      `^\\s*['"]?${escapeRegExp(key)}['"]?\\s*:`,
    );
    const index = lines.findIndex((line) => asMapKey.test(line));
    if (index >= 0) return index + 1;
  }
  for (const key of keys) {
    const index = lines.findIndex((line) => line.includes(key));
    if (index >= 0) return index + 1;
  }
  return 1;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
