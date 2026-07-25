import type { EditorTab } from "../../hooks/useWorkspaceLayout";

type EditorTabsProps = {
  tabs: EditorTab[];
  activeTabId: string;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
};

export function EditorTabs({
  tabs,
  activeTabId,
  onSelect,
  onClose,
}: EditorTabsProps) {
  return (
    <div className="tabs">
      {tabs.map((tab) => {
        const active = tab.id === activeTabId;

        return (
          <button
            key={tab.id}
            type="button"
            className={`tab ${active ? "active" : ""}`}
            onClick={() => onSelect(tab.id)}
          >
            {tab.title}
            {tab.dirty ? (
              <span className="dot-mod tab-dot" />
            ) : tabs.length > 1 ? (
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
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
