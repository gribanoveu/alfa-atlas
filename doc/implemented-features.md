# Реализованные возможности (as-built)

Актуальное описание того, что уже работает в **docflow** (Tauri + React + Rust), и как это устроено.  
Целевые бизнес-требования продукта (vision / backlog) — в [business-requirements/](./business-requirements/).

Идентификатор приложения: `com.eugene.docflow`. Стек: Tauri v2, React 19, TypeScript, Monaco, bun, Rust.

---

## 1. Оболочка IDE

Одноэкранный workspace без роутинга ([`src/App.tsx`](../src/App.tsx)):

| Область | Поведение |
|---------|-----------|
| **TopBar** | Бренд `docflow`, выпадающие меню (Файл…Справка), чипы репозитория / ветки. Имя репозитория — последний сегмент `repoRoot`; ветка — через `git2` (`get_git_branch`). |
| **Sidebar** | Панель «Документация»: дерево файлов от `docsRoot` с фильтром supported-форматов. |
| **Центр** | Без проекта — [Welcome](#4-стартовый-экран-welcome); с проектом — [редактор Monaco](#5-редактор). |
| **RightDock** | Инструменты: Ассистент, AsciiDoc, Git (тела панелей — заглушки). |
| **BottomDock** | Подсказки, Форматирование — заглушки. |
| **StatusBar** | Относительный путь файла от `docsRoot`, язык, UTF-8, курсор, «AI-индекс актуален». |

Сворачивание панелей — только сессия. Дизайн: тёмные токены JetBrains-like ([`src/styles/tokens.css`](../src/styles/tokens.css)).

---

## 2. Размер и позиция окна

| | |
|--|--|
| Файл | `~/.docflow/settings.json` |
| Поля | `window.width`, `height`, `x`, `y`, `maximized` |
| Код | [`domain/settings.rs`](../src-tauri/src/domain/settings.rs), [`services/window_settings.rs`](../src-tauri/src/services/window_settings.rs) |

---

## 3. Меню TopBar

| Меню | Поведение |
|------|-----------|
| **Файл** | Открыть папку… · Клонировать… (заглушка) · Сохранить · Закрыть проект · Выход |
| **Правка** | Пока disabled |
| **Вид** | Левая / правая / нижняя панели |
| **Инструменты** | Настройки… |
| **Навигация / Git** | Заглушки |
| **Справка** | О программе · Документация · Отзыв · Обновления |

Сохранение также по `⌘S` / `Ctrl+S`.

### Настройки

- **Общие:** «Открывать последний проект при запуске»; «Закрыть проект».
- **Редактор:** список поддерживаемых форматов.
- **Пути:** `~/.docflow`, путь репозитория, `{repo}/.docflow`.

---

## 4. Стартовый экран (Welcome)

1. **Открыть папку…** — выбор каталога → resolve git root → если нет валидного `{repo}/.docflow/project.json`, модалка подтверждения корня документации → открытие.
2. **Клонировать репозиторий…** — UI-заглушка (реального clone нет).
3. **Недавние** — placeholder.

---

## 5. Редактор

- Monaco (локальные воркеры) — [`src/monacoSetup.ts`](../src/monacoSetup.ts).
- Вкладки с диска: `path`, `content` / `savedContent`, `dirty = content !== savedContent`.
- Открытие файла — клик в дереве; сохранение — меню / ⌘S.
- Без открытых файлов — placeholder «Откройте файл в дереве».
- Закрытие dirty-вкладки — `window.confirm`.

Хук: [`src/hooks/useEditorTabs.ts`](../src/hooks/useEditorTabs.ts).

---

## 6. Проект: repoRoot + docsRoot

| | |
|--|--|
| Глобально | `~/.docflow/settings.json` → `project.root` = абсолютный **repoRoot** |
| В репозитории | `{repoRoot}/.docflow/project.json` → `{ "docsRoot": "src/docs/asciidoc" }` (относительный путь) |
| Layout | `{repoRoot}/.docflow/layout.json` |
| IPC | `probe_open_path`, `open_project`, `open_cached_project`, `get_project`, `get_saved_repo_root`, `clear_project`, `get_git_branch` |
| Frontend | [`src/lib/project.ts`](../src/lib/project.ts), [`src/hooks/useProject.ts`](../src/hooks/useProject.ts) |
| Backend | [`services/project_open.rs`](../src-tauri/src/services/project_open.rs), [`infra/project_store.rs`](../src-tauri/src/infra/project_store.rs), [`infra/git_repo.rs`](../src-tauri/src/infra/git_repo.rs) |

**Поведение**

- Есть валидный `project.json` → открытие без scan и без модалки.
- Нет / битый docs path → probe (эвристики имён + плотность supported-файлов) → confirm → запись `project.json`.
- Close очищает только global `project.root`; `.docflow` в репо остаётся.
- Дерево и read/write ограничены `docsRoot` (path containment).

---

## 7. Дерево и файлы

| IPC | Роль |
|-----|------|
| `list_docs_tree` | Дерево от docsRoot, только supported + папки к ним |
| `read_project_file` / `write_project_file` | Относительные пути под docsRoot |

UI: [`Sidebar`](../src/components/Sidebar/Sidebar.tsx) + [`FileTree`](../src/components/Sidebar/FileTree.tsx).

---

## 8. Ресайз панелей

| | |
|--|--|
| Файл | `{repoRoot}/.docflow/layout.json` |
| Поля | `sidebarWidth`, `rightWidth`, `bottomHeight` |
| Дефолты | 220 / 340 / 220 |

---

## 9. Поддерживаемые форматы

[`src/lib/supportedFiles.ts`](../src/lib/supportedFiles.ts) и [`domain/supported_files.rs`](../src-tauri/src/domain/supported_files.rs):

`.adoc` / `.asciidoc`, `.json`, `.md` / `.markdown`, `.txt`, `.puml` / `.plantuml`, `.yaml` / `.yml`, `.mmd` / `.mermaid`.

---

## 10. Что намеренно не реализовано

- реальный `git clone`, stage/commit/push/pull;
- AI-ассистент, AsciiDoc-библиотека, preview/split;
- недавние проекты;
- KPI «Покрытие документацией».

---

## 11. Архитектура кода

```
Frontend:  components → hooks → lib (IPC wrappers)
Rust:      commands → services → domain
           infra реализует I/O (settings / project / layout / git)
```

---

## 12. Файлы настроек (сводка)

| Файл | Назначение |
|------|------------|
| `app.config.json` | Версия и URL Справки |
| `~/.docflow/settings.json` | Окно; последний `project.root`; `general.restoreLastProject` |
| `{repo}/.docflow/project.json` | Относительный `docsRoot` |
| `{repo}/.docflow/layout.json` | Размеры панелей |

---

## 13. Запуск и проверки

```bash
bun install
bun run tauri dev
bun run tsc --noEmit
cd src-tauri && cargo check && cargo test
```

Package manager фронта — **bun**.
