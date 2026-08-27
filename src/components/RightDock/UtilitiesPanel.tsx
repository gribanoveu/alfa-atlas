import { UTILITIES, type UtilityDef, type UtilityId } from "../../data/utilities";
import "./UtilitiesPanel.css";

type UtilitiesPanelProps = {
  /** Открывает утилиту вкладкой в редакторе. */
  onOpen: (id: UtilityId) => void;
  /** Утилита, вкладка которой сейчас активна — подсвечивается в списке. */
  activeId: UtilityId | null;
};

type UtilityCardProps = {
  utility: UtilityDef;
  active: boolean;
  onOpen: (id: UtilityId) => void;
};

function UtilityCard({ utility, active, onOpen }: UtilityCardProps) {
  const { id, title, description, icon: Icon, stub } = utility;
  return (
    <button
      type="button"
      className={`utils-card${active ? " is-active" : ""}`}
      onClick={() => onOpen(id)}
      title={description}
    >
      <span className="utils-card-icon">
        <Icon size={14} strokeWidth={1.75} aria-hidden />
      </span>
      <span className="utils-card-body">
        <span className="utils-card-title">
          <span className="utils-card-label">{title}</span>
          {stub ? <span className="utils-card-badge">Скоро</span> : null}
        </span>
        <span className="utils-card-desc">{description}</span>
      </span>
    </button>
  );
}

export function UtilitiesPanel({ onOpen, activeId }: UtilitiesPanelProps) {
  return (
    <div className="utils-panel">
      {UTILITIES.length === 0 ? (
        <div className="panel-empty">Утилиты недоступны</div>
      ) : (
        <div className="utils-card-list">
          {UTILITIES.map((utility) => (
            <UtilityCard
              key={utility.id}
              utility={utility}
              active={utility.id === activeId}
              onOpen={onOpen}
            />
          ))}
        </div>
      )}
    </div>
  );
}
