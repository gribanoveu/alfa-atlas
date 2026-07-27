import {
  ArrowDownToLine,
  ArrowUpFromLine,
  GitCommitHorizontal,
  GitFork,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { appConfig } from "../../lib/appConfig";
import type { MenuActionId } from "../../lib/menuActions";
import { MenuDropdown, type MenuItem } from "./MenuDropdown";

type MenuDef = {
  id: string;
  label: string;
  items: MenuItem[];
};

const MENUS: MenuDef[] = [
  {
    id: "file",
    label: "Файл",
    items: [
      {
        type: "item",
        id: "open-folder",
        label: "Открыть папку…",
        action: "file.openFolder",
      },
      {
        type: "item",
        id: "clone",
        label: "Клонировать репозиторий…",
        action: "file.cloneRepo",
      },
      { type: "separator" },
      {
        type: "item",
        id: "save",
        label: "Сохранить",
        action: "file.save",
      },
      {
        type: "item",
        id: "close-project",
        label: "Закрыть проект",
        action: "file.closeProject",
      },
      { type: "separator" },
      { type: "item", id: "exit", label: "Выход", action: "file.exit" },
    ],
  },
  {
    id: "edit",
    label: "Правка",
    items: [
      { type: "item", id: "undo", label: "Отменить", disabled: true },
      { type: "item", id: "redo", label: "Повторить", disabled: true },
      { type: "separator" },
      { type: "item", id: "cut", label: "Вырезать", disabled: true },
      { type: "item", id: "copy", label: "Копировать", disabled: true },
      { type: "item", id: "paste", label: "Вставить", disabled: true },
    ],
  },
  {
    id: "view",
    label: "Вид",
    items: [
      {
        type: "item",
        id: "sidebar",
        label: "Панель документации",
        action: "view.toggleSidebar",
      },
      {
        type: "item",
        id: "right",
        label: "Правая панель",
        action: "view.toggleRight",
      },
      {
        type: "item",
        id: "bottom",
        label: "Нижняя панель",
        action: "view.toggleBottom",
      },
    ],
  },
  {
    id: "nav",
    label: "Навигация",
    items: [
      { type: "item", id: "goto", label: "Перейти к файлу…", disabled: true },
      {
        type: "item",
        id: "back",
        label: "Назад",
        action: "nav.goBack",
      },
      {
        type: "item",
        id: "forward",
        label: "Вперёд",
        action: "nav.goForward",
      },
    ],
  },
  {
    id: "git",
    label: "Git",
    items: [
      {
        type: "item",
        id: "commit",
        label: "Коммит / Commit…",
        action: "git.toggleCommit",
        icon: GitCommitHorizontal,
      },
      {
        type: "item",
        id: "create-branch",
        label: "Создать ветку…",
        action: "git.createBranch",
        icon: GitFork,
      },
      {
        type: "item",
        id: "pull",
        label: "Обновить проект",
        action: "git.pull",
        icon: ArrowDownToLine,
      },
      {
        type: "item",
        id: "push",
        label: "Отправить изменения",
        action: "git.push",
        icon: ArrowUpFromLine,
      },
    ],
  },
  {
    id: "tools",
    label: "Инструменты",
    items: [
      {
        type: "item",
        id: "settings",
        label: "Настройки…",
        action: "tools.settings",
      },
    ],
  },
  {
    id: "help",
    label: "Справка",
    items: [
      {
        type: "item",
        id: "about",
        label: `О программе — v${appConfig.version}`,
        action: "help.about",
      },
      {
        type: "item",
        id: "docs",
        label: "Документация",
        action: "help.docs",
      },
      {
        type: "item",
        id: "feedback",
        label: "Оставить отзыв",
        action: "help.feedback",
      },
      {
        type: "item",
        id: "updates",
        label: "Проверить обновления",
        action: "help.updates",
      },
    ],
  },
];

type MenuBarProps = {
  onAction: (action: MenuActionId) => void;
  hasProject?: boolean;
  gitBusy?: boolean;
  canGoBack?: boolean;
  canGoForward?: boolean;
};

export function MenuBar({
  onAction,
  hasProject = false,
  gitBusy = false,
  canGoBack = false,
  canGoForward = false,
}: MenuBarProps) {
  const [openId, setOpenId] = useState<string | null>(null);
  const rootRef = useRef<HTMLElement>(null);

  const menus = MENUS.map((menu) => {
    if (menu.id === "git") {
      return {
        ...menu,
        items: menu.items.map((item) =>
          item.type === "item"
            ? { ...item, disabled: !hasProject || gitBusy }
            : item,
        ),
      };
    }
    if (menu.id === "nav") {
      return {
        ...menu,
        items: menu.items.map((item): MenuItem => {
          if (item.type !== "item") return item;
          if (item.id === "back") {
            return { ...item, disabled: !canGoBack };
          }
          if (item.id === "forward") {
            return { ...item, disabled: !canGoForward };
          }
          return item;
        }),
      };
    }
    return menu;
  });

  useEffect(() => {
    if (!openId) return;

    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpenId(null);
      }
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpenId(null);
    };

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [openId]);

  return (
    <nav className="menu" ref={rootRef} aria-label="Главное меню">
      {menus.map((menu) => {
        const open = openId === menu.id;
        return (
          <div key={menu.id} className={`menu-root${open ? " is-open" : ""}`}>
            <button
              type="button"
              className={`menu-item${open ? " is-open" : ""}`}
              aria-haspopup="menu"
              aria-expanded={open}
              onClick={() => setOpenId(open ? null : menu.id)}
              onMouseEnter={() => {
                if (openId !== null) setOpenId(menu.id);
              }}
            >
              {menu.label}
            </button>
            {open ? (
              <MenuDropdown
                items={menu.items}
                onAction={(action) => {
                  setOpenId(null);
                  onAction(action);
                }}
              />
            ) : null}
          </div>
        );
      })}
    </nav>
  );
}
