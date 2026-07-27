import { X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  listRecentProjects,
  removeRecentProject,
  type RecentProject,
} from "../../lib/project";
import "./RecentProjectsDropdown.css";

type RecentProjectsDropdownProps = {
  onSelect: (root: string) => void;
  onClose: () => void;
};

export function RecentProjectsDropdown({
  onSelect,
  onClose,
}: RecentProjectsDropdownProps) {
  const [recent, setRecent] = useState<RecentProject[]>([]);
  const rootRef = useRef<HTMLDivElement>(null);

  const reload = useCallback(async () => {
    try {
      const items = await listRecentProjects();
      setRecent(items);
    } catch {
      setRecent([]);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (
        rootRef.current &&
        !rootRef.current.contains(event.target as Node)
      ) {
        onClose();
      }
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
  }, [onClose]);

  const handleRemove = async (root: string, event: React.MouseEvent) => {
    event.stopPropagation();
    try {
      await removeRecentProject(root);
      await reload();
    } catch {
      // silently ignore
    }
  };

  return (
    <div className="recent-projects-dropdown" ref={rootRef} role="menu">
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
