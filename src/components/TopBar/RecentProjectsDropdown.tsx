import { X } from "lucide-react";
import { useEffect, useRef } from "react";
import { useRecentProjectsList } from "../../hooks/useRecentProjects";
import "./RecentProjectsDropdown.css";

type RecentProjectsDropdownProps = {
  anchorRef: React.RefObject<HTMLElement | null>;
  onSelect: (root: string) => void;
  onClose: () => void;
};

export function RecentProjectsDropdown({
  anchorRef,
  onSelect,
  onClose,
}: RecentProjectsDropdownProps) {
  const { recent, removeRecent } = useRecentProjectsList();
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (anchorRef.current?.contains(target) || menuRef.current?.contains(target)) {
        return;
      }
      onClose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [anchorRef, onClose]);

  const handleRemove = (root: string, event: React.MouseEvent) => {
    event.stopPropagation();
    void removeRecent(root);
  };

  return (
    <div className="recent-projects-dropdown" ref={menuRef} role="menu">
      <div className="recent-projects-header">Недавние проекты</div>
      {recent.length === 0 ? (
        <div className="recent-projects-empty">Список пуст</div>
      ) : (
        <ul className="recent-projects-list">
          {recent.map((item) => (
            <li key={item.root} className="recent-projects-item">
              <button
                type="button"
                className="recent-projects-open"
                onClick={() => onSelect(item.root)}
              >
                <span className="recent-projects-name">{item.name}</span>
                <span className="recent-projects-path">{item.root}</span>
              </button>
              <button
                type="button"
                className="recent-projects-remove"
                aria-label={`Убрать «${item.name}» из недавних`}
                onClick={(e) => void handleRemove(item.root, e)}
              >
                <X size={14} aria-hidden />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
