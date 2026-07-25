# Реализованные возможности (as-built)

Актуальное описание того, что уже работает в **docflow** (Tauri + React + Rust), и как это устроено.  
Целевые бизнес-требования продукта (vision / backlog) — в [business-requirements/](./business-requirements/).

Идентификатор приложения: `com.eugene.docflow`. Стек: Tauri v2, React 19, TypeScript, Monaco, bun, Rust.

---

## 1. Оболочка IDE

Одноэкранный workspace без роутинга ([`src/App.tsx`](../src/App.tsx)):

| Область | Поведение |
|---------|-----------|
| **TopBar** | Бренд `docflow`, нерабочее меню (Файл…Справка), чипы репозитория / ветки. Имя папки проекта подставляется после открытия; ветка пока всегда `—`. |
| **Sidebar** | Панель «Документация»: сворачивается в rail 36px. Без дерева файлов — placeholder («Нет открытого репозитория» или имя/путь проекта). |
| **Центр** | Без проекта — [Welcome](#3-стартовый-экран-welcome); с проектом — [редактор Monaco](#4-редактор). |
| **RightDock** | Инструменты: Ассистент, AsciiDoc, Git (иконки на stripe ~40px). Тела панелей пустые (заглушки). |
| **BottomDock** | Подсказки, Форматирование — пустые заглушки. Свернут по умолчанию; открытая высота настраивается. |
| **StatusBar** | Путь/вкладка, язык, UTF-8, курсор (`Ln/Col`), статичный текст «AI-индекс актуален». |

Сворачивание панелей хранится только в сессии (не пишется на диск).

Дизайн: тёмные токены JetBrains-like ([`src/styles/tokens.css`](../src/styles/tokens.css)), сетка ([`src/styles/app.css`](../src/styles/app.css)).

---

## 2. Размер и позиция окна

При каждом запуске окно восстанавливается из пользовательских настроек; при ресайзе / перемещении / закрытии состояние сохраняется.

| | |
|--|--|
| Файл | `~/.docflow/settings.json` |
| Поля | `window.width`, `height`, `x`, `y`, `maximized` |
| Минимум / дефолт | 800×600 |
| Код | [`domain/settings.rs`](../src-tauri/src/domain/settings.rs), [`services/window_settings.rs`](../src-tauri/src/services/window_settings.rs), [`lib.rs`](../src-tauri/src/lib.rs) |

Пока окно развёрнуто, «обычные» размер и позиция не перезаписываются — после снятия maximize / при следующем старте возвращаются прежние.

Окно создаётся скрытым (`visible: false` в `tauri.conf.json`), затем показывается после применения сохранённого состояния (без мигания дефолтным размером).

---

## 3. Стартовый экран (Welcome)

Показывается, если нет открытого проекта (`project.root` отсутствует или путь больше не существует).

Действия:

1. **Открыть папку…** — системный диалог выбора каталога (`tauri-plugin-dialog`) → сохранение корня проекта → Welcome скрывается, появляется редактор.
2. **Клонировать репозиторий…** — модалка URL + папка назначения. **Реального `git clone` нет** — после подтверждения показывается сообщение-заглушка ([`CloneRepoModal.tsx`](../src/components/Welcome/CloneRepoModal.tsx)).
3. **Недавние** — пустой placeholder (список недавних проектов не реализован).

Иконки действий — `lucide-react`.

---

## 4. Редактор

- Monaco Editor (локальный бандл воркеров, без CDN) — [`src/monacoSetup.ts`](../src/monacoSetup.ts).
- Вкладки в памяти: старт с `Untitled-1`; dirty-индикатор; последнюю вкладку закрыть нельзя.
- Курсор транслируется в StatusBar.
- **Нет** чтения/записи файлов с диска, дерева, breadcrumb, режимов Split/Preview.

Хук состояния вкладок: [`src/hooks/useWorkspaceLayout.ts`](../src/hooks/useWorkspaceLayout.ts).

---

## 5. Открытие проекта

| | |
|--|--|
| Хранение корня | `~/.docflow/settings.json` → `project.root` |
| IPC | `get_project_root`, `set_project_root`, `clear_project_root` |
| Frontend | [`src/lib/project.ts`](../src/lib/project.ts), [`src/hooks/useProject.ts`](../src/hooks/useProject.ts) |
| Backend | [`commands/project.rs`](../src-tauri/src/commands/project.rs), [`services/project_settings.rs`](../src-tauri/src/services/project_settings.rs) |

При старте приложения путь проверяется: если каталога нет — корень сбрасывается, показывается Welcome.  
`closeProject` в хуке есть, в UI кнопки «закрыть проект» пока нет.

Capability: `dialog:default` в [`capabilities/default.json`](../src-tauri/capabilities/default.json).

---

## 6. Ресайз панелей и per-project layout

Левую, правую и нижнюю панели можно тянуть за край (как в VS Code / IDEA), когда они **открыты**.

| | |
|--|--|
| Файл | `{projectRoot}/.docflow/layout.json` |
| Поля | `sidebarWidth`, `rightWidth`, `bottomHeight` |
| Дефолты | 220 / 340 / 220 |
| Клампа | sidebar 160–480, right 200–560, bottom 120–480 |

**Правила сохранения**

- Проект открыт → load при открытии, save при окончании drag (~150 ms debounce).
- Проекта нет → всегда дефолты при «сбросе» (закрытии проекта / старте без root); drag в сессии возможен, **на диск не пишется**.

Код: [`domain/layout.rs`](../src-tauri/src/domain/layout.rs), [`infra/layout_store.rs`](../src-tauri/src/infra/layout_store.rs), [`src/hooks/usePanelLayout.ts`](../src/hooks/usePanelLayout.ts), [`PanelResizeHandle`](../src/components/PanelResizeHandle/PanelResizeHandle.tsx).

Глобальный `~/.docflow/settings.json` для размеров панелей **не используется**.

---

## 7. Поддерживаемые форматы файлов

Объявлены в [`src/lib/supportedFiles.ts`](../src/lib/supportedFiles.ts) и в [бизнес-правилах §4.3](./business-requirements/04-business-rules.md):

`.adoc` / `.asciidoc`, `.json`, `.md` / `.markdown`, `.txt`, `.puml` / `.plantuml`, `.yaml` / `.yml`, `.mmd` / `.mermaid`.

Сейчас список — контракт для будущих open/save и фильтров. **Открытие файлов с диска ещё не подключено**, хелперы `isSupportedFile` / `monacoLanguageFor` пока не используются UI.

---

## 8. Что намеренно не реализовано

См. также [05-integrations-and-scope.md](./business-requirements/05-integrations-and-scope.md) (целевой backlog). В текущем приложении нет:

- дерева документации и работы с файлами репозитория;
- реального Git (stage/commit/clone/branch);
- AI-ассистента, AsciiDoc-библиотеки, подсказок / форматирования с данными;
- preview / split;
- рабочих пунктов верхнего меню;
- списка недавних проектов;
- закрытия проекта из UI.

---

## 9. Архитектура кода

Dependency direction (см. [AGENTS.md](../AGENTS.md)):

```
Frontend:  components → hooks → lib (IPC wrappers)
Rust:      commands → services → domain
           infra реализует I/O (settings / layout store)
```

| Слой | Пути |
|------|------|
| Commands | `src-tauri/src/commands/` |
| Services | `src-tauri/src/services/` |
| Domain | `src-tauri/src/domain/` |
| Infra | `src-tauri/src/infra/` |
| UI | `src/components/`, `src/hooks/`, `src/lib/` |

---

## 10. Файлы настроек (сводка)

| Файл | Назначение |
|------|------------|
| `~/.docflow/settings.json` | Размер/позиция/maximize окна; `project.root` |
| `{project}/.docflow/layout.json` | Ширины/высота панелей для этого проекта |

---

## 11. Запуск и проверки

```bash
bun install
bun run tauri dev          # приложение
bun run tsc --noEmit       # фронт
cd src-tauri && cargo check && cargo test
```

Package manager фронта — **bun** (не npm/pnpm/yarn).
