import { useEffect } from "react";
import { SKILLS_GUIDE_MD } from "../../lib/skillsGuide";
import { AssistantMarkdown } from "../RightDock/AssistantMarkdown";
import "../ToolLog/ToolCallLogModal.css";
import "./SkillsGuideModal.css";

type SkillsGuideModalProps = {
  onClose: () => void;
};

/** Как написать скил и как его установить — статический текст из
 * `lib/skillsGuide`, отрендеренный тем же Markdown, что и чат. */
export function SkillsGuideModal({ onClose }: SkillsGuideModalProps) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <div className="tool-log-backdrop" role="presentation" onClick={onClose}>
      <div
        className="tool-log-dialog skills-guide-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="skills-guide-title"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="tool-log-header">
          <h2 className="tool-log-title" id="skills-guide-title">
            Как написать и установить скил
          </h2>
          <div className="tool-log-header-actions">
            <button type="button" className="tool-log-close" onClick={onClose} aria-label="Закрыть">
              ×
            </button>
          </div>
        </header>
        <div className="skills-guide-scroll">
          <AssistantMarkdown content={SKILLS_GUIDE_MD} streaming={false} />
        </div>
      </div>
    </div>
  );
}
