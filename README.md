# docflow

Desktop-редактор документации, работающий с git-репозиториями.

- Идентификатор: `com.eugene.docflow`
- Стек: **Tauri v2**, React + TypeScript, Rust
- Package manager: **bun**

## Документация

| Документ | Содержание |
|----------|------------|
| [doc/implemented-features.md](./doc/implemented-features.md) | **Что уже реализовано** и как это работает |
| [doc/business-requirements/](./doc/business-requirements/) | Целевые бизнес-требования (vision / backlog) |
| [AGENTS.md](./AGENTS.md) | Конвенции для AI-агентов и архитектура слоёв |

## Быстрый старт

```bash
bun install
bun run tauri dev
```

Проверки:

```bash
bun run tsc --noEmit
cd src-tauri && cargo check
```

## Что умеет сейчас (кратко)

- IDE-оболочка: top bar с меню, боковые и нижняя панели, status bar, Monaco-редактор
- Welcome без проекта: открыть папку (рабочее), клонировать (UI-заглушка)
- Справка: версия и ссылки из [`app.config.json`](./app.config.json)
- Сохранение окна в `~/.docflow/settings.json`
- Размеры панелей на проект в `{project}/.docflow/layout.json`

Подробности — в [implemented-features.md](./doc/implemented-features.md).
