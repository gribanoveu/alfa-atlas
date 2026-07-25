import type { MenuActionId } from "../../lib/menuActions";

export type MenuItem =
  | { type: "separator" }
  | {
      type: "item";
      id: string;
      label: string;
      disabled?: boolean;
      action?: MenuActionId;
    };

type MenuDropdownProps = {
  items: MenuItem[];
  onAction: (action: MenuActionId) => void;
};

export function MenuDropdown({ items, onAction }: MenuDropdownProps) {
  return (
    <div className="menu-dropdown" role="menu">
      {items.map((item, index) => {
        if (item.type === "separator") {
          return <div key={`sep-${index}`} className="menu-dropdown-sep" />;
        }

        return (
          <button
            key={item.id}
            type="button"
            role="menuitem"
            className="menu-dropdown-item"
            disabled={item.disabled || !item.action}
            onClick={() => {
              if (item.action && !item.disabled) onAction(item.action);
            }}
          >
            {item.label}
          </button>
        );
      })}
    </div>
  );
}
