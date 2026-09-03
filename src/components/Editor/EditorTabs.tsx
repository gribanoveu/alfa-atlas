import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import { Columns2, Code, Eye } from "lucide-react";
import type { EditorViewMode } from "../../types/viewMode";

/** Minimal shape the tab strip needs to render a tab — satisfied by real
 * file tabs (`EditorTab`) as well as non-file pseudo-tabs (e.g. the API
 * Explorer), which don't carry editor content/dirty-state machinery. */
export type DisplayTab = {
  id: string;
  title: string;
  dirty: boolean;
};

type EditorTabsProps = {
  tabs: DisplayTab[];
  activeTabId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onCloseAll: () => void;
  onCloseOthers: (id: string) => void;
  viewMode: EditorViewMode;
  onViewModeChange: (mode: EditorViewMode) => void;
  /** Hide source/split/render switch (e.g. image preview tabs). */
  hideViewMode?: boolean;
  /** Subset of view modes to offer; defaults to all three. */
  allowedViewModes?: readonly EditorViewMode[];
  /** Extra controls rendered before the view-mode switch (e.g. plan «Начать»). */
  actions?: ReactNode;
};

const VIEW_MODE_BUTTONS: {
  mode: EditorViewMode;
  icon: typeof Code;
  label: string;
}[] = [
  { mode: "source", icon: Code, label: "Исходный код" },
  { mode: "split", icon: Columns2, label: "Сплит" },
  { mode: "render", icon: Eye, label: "Рендер" },
];

type ContextMenuState = {
  x: number;
  y: number;
  tabId: string;
};

const MENU_WIDTH = 220;
const MENU_HEIGHT = 108;

export function EditorTabs({
  tabs,
  activeTabId,
  onSelect,
  onClose,
  onCloseAll,
  onCloseOthers,
  viewMode,
  onViewModeChange,
  hideViewMode = false,
  allowedViewModes,
  actions,
}: EditorTabsProps) {
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const modeSwitch =
    hideViewMode ? null : (
      <ViewModeSwitch
        viewMode={viewMode}
        onViewModeChange={onViewModeChange}
        allowedModes={allowedViewModes}
      />
    );

  const trailing =
    actions || modeSwitch ? (
      <div className="tabs-bar-trailing">
        {actions}
        {modeSwitch}
      </div>
    ) : null;

  useLayoutEffect(() => {
    if (!menu || !menuRef.current) return;
    const rect = menuRef.current.getBoundingClientRect();
    const maxX = window.innerWidth - rect.width - 4;
    const maxY = window.innerHeight - rect.height - 4;
    const x = Math.max(4, Math.min(menu.x, maxX));
    const y = Math.max(4, Math.min(menu.y, maxY));
    if (x !== menu.x || y !== menu.y) {
      setMenu({ ...menu, x, y });
    }
  }, [menu]);

  useEffect(() => {
    if (!menu) return;

    const onPointerDown = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) {
        setMenu(null);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setMenu(null);
    };
    const onScroll = () => setMenu(null);

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [menu]);

  if (tabs.length === 0) {
    return (
      <div className="tabs-bar">
        <div className="tabs tabs-empty" />
        {trailing}
      </div>
    );
  }

  const openContextMenu = (event: ReactMouseEvent, tabId: string) => {
    event.preventDefault();
    event.stopPropagation();
    window.getSelection()?.removeAllRanges();
    const x = Math.min(event.clientX, window.innerWidth - MENU_WIDTH - 4);
    const y = Math.min(event.clientY, window.innerHeight - MENU_HEIGHT - 4);
    setMenu({ x: Math.max(4, x), y: Math.max(4, y), tabId });
    onSelect(tabId);
  };

  return (
    <div className="tabs-bar">
      <div className="tabs">
        {tabs.map((tab) => {
          const active = tab.id === activeTabId;

          return (
            <button
              key={tab.id}
              type="button"
              className={`tab ${active ? "active" : ""}`}
              onClick={() => onSelect(tab.id)}
              onContextMenu={(event) => openContextMenu(event, tab.id)}
              // The label is capped, so the full one has to be reachable
              // somewhere — a file tab's title is a name, but an artifact's
              // is a sentence the user wrote.
              title={tab.title}
            >
              <span className="tab-title">{tab.title}</span>
              {tab.dirty ? <span className="dot-mod tab-dot" /> : null}
              <span
                className="close"
                role="button"
                tabIndex={0}
                onClick={(event) => {
                  event.stopPropagation();
                  onClose(tab.id);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.stopPropagation();
                    onClose(tab.id);
                  }
                }}
              >
                ×
              </span>
            </button>
          );
        })}

        {menu ? (
          <div
            ref={menuRef}
            className="tab-context-menu"
            role="menu"
            style={{ left: menu.x, top: menu.y }}
          >
            <button
              type="button"
              role="menuitem"
              className="tab-context-item"
              onClick={() => {
                onClose(menu.tabId);
                setMenu(null);
              }}
            >
              Закрыть
            </button>
            <button
              type="button"
              role="menuitem"
              className="tab-context-item"
              onClick={() => {
                onCloseAll();
                setMenu(null);
              }}
            >
              Закрыть все
            </button>
            <button
              type="button"
              role="menuitem"
              className="tab-context-item"
              disabled={tabs.length <= 1}
              onClick={() => {
                onCloseOthers(menu.tabId);
                setMenu(null);
              }}
            >
              Закрыть все, кроме этой
            </button>
          </div>
        ) : null}
      </div>
      {trailing}
    </div>
  );
}

function ViewModeSwitch({
  viewMode,
  onViewModeChange,
  allowedModes,
}: {
  viewMode: EditorViewMode;
  onViewModeChange: (mode: EditorViewMode) => void;
  allowedModes?: readonly EditorViewMode[];
}) {
  const buttons = allowedModes
    ? VIEW_MODE_BUTTONS.filter((b) => allowedModes.includes(b.mode))
    : VIEW_MODE_BUTTONS;

  return (
    <div className="view-mode-switch" role="group" aria-label="Режим просмотра">
      {buttons.map(({ mode, icon: Icon, label }) => (
        <button
          key={mode}
          type="button"
          className={`view-btn ${viewMode === mode ? "active" : ""}`}
          onClick={() => onViewModeChange(mode)}
          title={label}
          aria-pressed={viewMode === mode}
          aria-label={label}
        >
          <Icon size={14} />
        </button>
      ))}
    </div>
  );
}
