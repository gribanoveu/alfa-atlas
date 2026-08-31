# Alfa Atlas

Desktop-редактор документации, работающий с git-репозиториями.

- Идентификатор: `com.eugene.alfa-atlas`
- Стек: **Tauri v2**, React + TypeScript, Rust
- Package manager: **bun**

## Установка

Готовые сборки — на странице [Releases](https://github.com/gribanoveu/alfa-atlas/releases).
Для macOS выкладывается `.dmg` (Apple Silicon), для Windows — `.msi` и `.exe`.

### macOS: «Не удалось проверить разработчика»

Сборки подписаны ad-hoc — то есть без сертификата Apple Developer, поэтому
Gatekeeper не пропускает их молча. При первом запуске:

1. Дважды кликните приложение и закройте появившееся предупреждение.
2. Откройте *Системные настройки → Конфиденциальность и безопасность*.
3. Внизу, в блоке «Безопасность», нажмите **«Подтвердить открытие»**
   («Open Anyway») рядом с названием приложения.

Подтвердить нужно один раз — дальше приложение запускается обычным способом.

### macOS: «Приложение повреждено» (сборки v0.3.0 и старше)

Приложение не повреждено. В релизах до v0.3.0 включительно подписи не было
вообще, и Gatekeeper показывал на такие сборки именно это сообщение — кнопка
«Подтвердить открытие» для них не появляется. Снимите атрибут карантина
вручную:

```bash
xattr -dr com.apple.quarantine "/Applications/Alfa Atlas.app"
```

Путь подставьте свой, если приложение лежит не в `/Applications`.

### Windows

Неподписанный установщик вызывает предупреждение SmartScreen — нажмите
«Подробнее» → «Выполнить в любом случае».

## Документация

| Документ | Содержание |
|----------|------------|
| [doc/implemented-features.md](./doc/implemented-features.md) | **Что уже реализовано** и как это работает |
| [doc/business-requirements/](./doc/business-requirements/) | Целевые бизнес-требования (vision / backlog) |
| [doc/releasing.md](./doc/releasing.md) | Как выпустить релиз: версии, тег, CI |
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

### Windows: Strawberry Perl для сборки

На Windows `libssh2` собирается с бэкендом OpenSSL (см. секцию
`[target.'cfg(windows)'.dependencies]` в [src-tauri/Cargo.toml](./src-tauri/Cargo.toml)):
бэкенд WinCNG, который берётся по умолчанию, не умеет ни ed25519, ни ключи в
формате OpenSSH, и клон по SSH с ним зависает. OpenSSL собирается из исходников,
а его `Configure` написан на Perl, поэтому локально нужен
[Strawberry Perl](https://strawberryperl.com/) — msys-perl из Git for Windows не
подходит (в нём нет `Locale::Maketext::Simple`). В CI он уже есть на
`windows-latest`.

## Что умеет сейчас (кратко)

- IDE-оболочка: top bar с меню, боковые и нижняя панели, status bar, Monaco-редактор
- Welcome без проекта: открыть папку (рабочее), клонировать (UI-заглушка)
- Справка: версия и ссылки из [`app.config.json`](./app.config.json)
- Сохранение окна в `~/.docflow/settings.json`
- Размеры панелей на проект в `{project}/.docflow/layout.json`

Подробности — в [implemented-features.md](./doc/implemented-features.md).
