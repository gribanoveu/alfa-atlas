# Alfa Atlas

Desktop-редактор документации, работающий с git-репозиториями.

- Идентификатор: `com.eugene.alfa-atlas`
- Стек: **Tauri v2**, React + TypeScript, Rust
- Package manager: **bun**

## Установка

Готовые сборки — на странице [Releases](https://github.com/gribanoveu/alfa-atlas/releases).
Для macOS выкладывается `.dmg` (Apple Silicon), для Windows — `.msi` и `.exe`.

### macOS: «Приложение повреждено, и его не удаётся открыть»

Приложение не повреждено. Сборки не подписаны сертификатом Apple Developer,
а macOS вешает на всё скачанное из интернета атрибут `com.apple.quarantine`.
Увидев карантин на приложении без подписи, Gatekeeper показывает именно это
сообщение вместо более честного «неопознанный разработчик».

Снимите атрибут карантина:

```bash
xattr -dr com.apple.quarantine "/Applications/Alfa Atlas.app"
```

После этого приложение запустится. Путь подставьте свой, если приложение
лежит не в `/Applications`.

Кнопка «Всё равно открыть» в *Системные настройки → Конфиденциальность и
безопасность* в этом случае обычно не помогает: она рассчитана на подписанные
приложения от неопознанного разработчика, а здесь подписи нет вовсе.

Windows на неподписанном установщике показывает предупреждение SmartScreen —
там достаточно «Подробнее» → «Выполнить в любом случае».

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
