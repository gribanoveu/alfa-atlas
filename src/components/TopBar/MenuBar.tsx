import {
  ArrowDownToLine,
  ArrowUpFromLine,
  BookOpen,
  Brain,
  Clipboard,
  Copy,
  FileInput,
  FolderOpen,
  FolderX,
  GitBranch,
  GitCommitHorizontal,
  GitFork,
  Info,
  ListTodo,
  LogOut,
  MessageSquare,
  PanelBottom,
  PanelLeft,
  PanelRight,
  Redo2,
  RefreshCw,
  Save,
  Scissors,
  ScrollText,
  Search,
  Settings2,
  Undo2,
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
        icon: FolderOpen,
      },
      { type: "separator" },
      {
        type: "item",
        id: "save",
        label: "Сохранить",
        action: "file.save",
        icon: Save,
      },
      {
        type: "item",
        id: "close-project",
        label: "Закрыть проект",
        action: "file.closeProject",
        icon: FolderX,
      },
      { type: "separator" },
      { type: "item", id: "exit", label: "Выход", action: "file.exit", icon: LogOut },
    ],
  },
  {
    id: "edit",
    label: "Правка",
    items: [
      { type: "item", id: "undo", label: "Отменить", action: "edit.undo", icon: Undo2 },
      { type: "item", id: "redo", label: "Повторить", action: "edit.redo", icon: Redo2 },
      { type: "separator" },
      { type: "item", id: "cut", label: "Вырезать", disabled: true, icon: Scissors },
      { type: "item", id: "copy", label: "Копировать", disabled: true, icon: Copy },
      { type: "item", id: "paste", label: "Вставить", disabled: true, icon: Clipboard },
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
        icon: PanelLeft,
      },
      {
        type: "item",
        id: "right",
        label: "Правая панель",
        action: "view.toggleRight",
        icon: PanelRight,
      },
      {
        type: "item",
        id: "bottom",
        label: "Нижняя панель",
        action: "view.toggleBottom",
        icon: PanelBottom,
      },
    ],
  },
  {
    id: "git",
    label: "Git",
    items: [
      {
        type: "item",
        id: "clone",
        label: "Клонировать репозиторий…",
        action: "git.cloneRepo",
        icon: GitBranch,
      },
      { type: "separator" },
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
        id: "find-in-docs",
        label: "Найти в документации…",
        action: "nav.findInDocs",
        icon: Search,
      },
      { type: "separator" },
      {
        type: "item",
        id: "settings",
        label: "Настройки…",
        action: "tools.settings",
        icon: Settings2,
      },
      {
        type: "item",
        id: "toolLog",
        label: "Журнал вызовов…",
        action: "tools.toolLog",
        icon: ScrollText,
      },
      {
        type: "item",
        id: "memoryLog",
        label: "Память ассистента…",
        action: "tools.memoryLog",
        icon: Brain,
      },
      {
        type: "item",
        id: "plans",
        label: "Планы…",
        action: "tools.plans",
        icon: ListTodo,
      },
      {
        type: "item",
        id: "artifacts",
        label: "Артефакты…",
        action: "tools.artifacts",
        icon: FileInput,
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
        icon: Info,
      },
      {
        type: "item",
        id: "docs",
        label: "Документация",
        action: "help.docs",
        icon: BookOpen,
      },
      {
        type: "item",
        id: "feedback",
        label: "Оставить отзыв",
        action: "help.feedback",
        icon: MessageSquare,
      },
      {
        type: "item",
        id: "updates",
        label: "Проверить обновления",
        action: "help.updates",
        icon: RefreshCw,
      },
    ],
  },
];

type MenuBarProps = {
  onAction: (action: MenuActionId) => void;
  hasProject?: boolean;
  gitBusy?: boolean;
  hasActiveTab?: boolean;
};

export function MenuBar({
  onAction,
  hasProject = false,
  gitBusy = false,
  hasActiveTab = false,
}: MenuBarProps) {
  const [openId, setOpenId] = useState<string | null>(null);
  const rootRef = useRef<HTMLElement>(null);

  const menus = MENUS.map((menu) => {
    if (menu.id === "git") {
      return {
        ...menu,
        items: menu.items.map((item) => {
            if (item.type !== "item") return item;
            if (item.id === "clone") return item;
            return { ...item, disabled: !hasProject || gitBusy };
          }),
      };
    }
    if (menu.id === "edit") {
      return {
        ...menu,
        items: menu.items.map((item) => {
          if (item.type !== "item") return item;
          if (item.id !== "undo" && item.id !== "redo") return item;
          // Monaco's ITextModel has no public canUndo/canRedo — gate on
          // "is a document open" rather than the exact stack state; an
          // Undo/Redo click with nothing to do is a harmless no-op.
          return { ...item, disabled: !hasActiveTab };
        }),
      };
    }
    if (menu.id === "tools") {
      return {
        ...menu,
        items: menu.items.map((item) => {
          if (item.type !== "item") return item;
          if (item.id === "find-in-docs") {
            return { ...item, disabled: !hasProject };
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
