import { FolderOpen } from "lucide-react";
import { ARTIFACT_KINDS } from "../../data/artifactKinds";
import { UTILITIES, type UtilityDef, type UtilityId } from "../../data/utilities";
import type { ArtifactKind } from "../../lib/artifacts";
import "./UtilitiesPanel.css";

type UtilitiesPanelProps = {
  /** Открывает утилиту вкладкой в редакторе. */
  onOpen: (id: UtilityId) => void;
  /** Утилита, вкладка которой сейчас активна — подсвечивается в списке. */
  activeId: UtilityId | null;
  /** Создаёт новый черновик артефакта нужного типа и открывает конструктор. */
  onNewArtifact: (kind: ArtifactKind) => void;
  /** Открывает список сохранённых артефактов. */
  onOpenArtifacts: () => void;
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

export function UtilitiesPanel({
  onOpen,
  activeId,
  onNewArtifact,
  onOpenArtifacts,
}: UtilitiesPanelProps) {
  return (
    <div className="utils-panel">
      {/* Artifacts sit above the utilities rather than among them: a
          utility is a stateless converter, an artifact is a saved document
          with an identity, so «новый» and «все» are the actions, not a
          single tool to open. One card per `ARTIFACT_KINDS` entry — a new
          kind only needs a row in that registry, not a new button here. */}
      <div className="utils-section">
        <div className="utils-section-title">Артефакты</div>
        <div className="utils-card-list">
          {ARTIFACT_KINDS.map((kind) => (
            <button
              key={kind.id}
              type="button"
              className="utils-card"
              onClick={() => onNewArtifact(kind.id)}
            >
              <span className="utils-card-icon">
                <kind.icon size={14} strokeWidth={1.75} aria-hidden />
              </span>
              <span className="utils-card-body">
                <span className="utils-card-title">
                  <span className="utils-card-label">{kind.cardTitle}</span>
                </span>
                <span className="utils-card-desc">{kind.cardDescription}</span>
              </span>
            </button>
          ))}
          <button type="button" className="utils-card" onClick={onOpenArtifacts}>
            <span className="utils-card-icon">
              <FolderOpen size={14} strokeWidth={1.75} aria-hidden />
            </span>
            <span className="utils-card-body">
              <span className="utils-card-title">
                <span className="utils-card-label">Сохранённые артефакты</span>
              </span>
              <span className="utils-card-desc">Открыть или удалить ранее собранные</span>
            </span>
          </button>
        </div>
      </div>
      <div className="utils-section-title">Утилиты</div>
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
