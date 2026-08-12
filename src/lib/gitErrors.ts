/** Translates raw backend error strings (libgit2 messages, GitError Display
 * text — mostly English) into actionable Russian messages for an otherwise
 * fully localized UI. Applied at every surface that shows a raw git error:
 * the commit panel's inline banner (GitPanel.tsx) and the shared
 * AlertOkModal render site in App.tsx — so push/pull/reset/branch-delete
 * failures get the same treatment as commit failures instead of leaking
 * strings like "authentication failed: ..." untranslated. */
const CYRILLIC = /[а-яёА-ЯЁ]/;

export function friendlyGitError(error: string): string {
  // Already a human-authored Russian message — either crafted in the
  // frontend (e.g. "Не удалось сохранить открытые файлы...") or a backend
  // GitError::Message that's already localized (e.g. the stash
  // branch-mismatch guard, or the full-sync-active guard). Only raw
  // English backend/libgit2 text needs translating below.
  if (CYRILLIC.test(error)) {
    return error;
  }

  const lower = error.toLowerCase();

  if (
    lower.includes("user.name") ||
    lower.includes("user.email") ||
    lower.includes("missing identity")
  ) {
    return "Не заданы имя и email автора git (user.name / user.email). Попросите разработчика настроить или настройте сами в терминале.";
  }
  if (
    lower.includes("nothing staged") ||
    lower.includes("empty message") ||
    lower.includes("commit message is empty")
  ) {
    return "Нужно добавить файл в Stage и написать краткое описание.";
  }
  if (
    lower.startsWith("no_ssh_credentials:") ||
    lower.includes("no credentials available") ||
    lower.includes("authentication failed")
  ) {
    return "Не удалось подключиться к серверу: не настроена аутентификация (SSH-ключ). Откройте Настройки → Git → SSH-ключи, чтобы добавить или проверить ключ.";
  }
  if (lower.includes("no upstream")) {
    return "У текущей ветки нет upstream-ветки на сервере. Выполните Push, чтобы создать её.";
  }
  if (lower.includes("merge conflict")) {
    return "Обнаружен конфликт слияния — разрешите конфликты в панели Git и повторите попытку.";
  }
  if (lower.includes("rebase conflict")) {
    return "Обнаружен конфликт при перебазировании — разрешите конфликты в панели Git и повторите попытку.";
  }
  if (lower.includes("branch already exists")) {
    return "Ветка с таким именем уже существует.";
  }
  if (lower.includes("cannot delete the current branch")) {
    return "Нельзя удалить текущую ветку — сначала переключитесь на другую.";
  }
  if (lower.includes("destination already exists") || lower.includes("destination directory is not empty")) {
    return "Папка назначения уже существует и не пуста.";
  }
  if (lower.includes("not a git repository")) {
    return "Это не git-репозиторий.";
  }
  return `Не удалось: ${error}`;
}
