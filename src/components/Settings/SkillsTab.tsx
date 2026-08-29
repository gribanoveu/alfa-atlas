import { ChevronDown, ChevronRight, Eye } from "lucide-react";
import { useState } from "react";
import { useSkills } from "../../hooks/useSkills";
import type { SkillListItem } from "../../lib/skills";
import { SkillPreviewModal } from "./SkillPreviewModal";
import "../Welcome/CloneRepoModal.css";
import "./SkillsTab.css";

function skillKey(item: SkillListItem): string {
  return `${item.source}:${item.name}`;
}

export function SkillsTab() {
  const { items, error, busy, toggle, addSkill, removeSkill, openFolder } = useSkills();
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [previewed, setPreviewed] = useState<SkillListItem | null>(null);

  const toggleExpanded = (key: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  return (
    <>
      <div className="skills-toolbar">
        <button type="button" className="settings-btn" disabled={busy} onClick={() => void addSkill()}>
          Добавить скил
        </button>
        <button type="button" className="settings-link-btn" onClick={() => void openFolder()}>
          Открыть папку
        </button>
      </div>
      <div className="skills-list">
        {items === null ? (
          <p className="settings-hint">Загрузка…</p>
        ) : items.length === 0 ? (
          <p className="settings-hint">Нет скилов</p>
        ) : (
          items.map((item) => {
            const invalid = item.error != null;
            const key = skillKey(item);
            const isOpen = expanded.has(key);
            const detail = (invalid ? item.error : item.description) ?? "";
            const hasDetail = detail.trim().length > 0;
            const Chevron = isOpen ? ChevronDown : ChevronRight;

            return (
              <div
                key={key}
                className={`skills-row${invalid ? " is-invalid" : ""}${isOpen ? " is-open" : ""}`}
              >
                <div className="skills-row-main">
                  <label className="settings-check">
                    <input
                      type="checkbox"
                      checked={item.enabled && !invalid}
                      disabled={busy || invalid}
                      onChange={(event) => void toggle(item, event.target.checked)}
                    />
                    <span className="skills-row-title">
                      <span className="skills-name">{item.name}</span>
                      <span className="skills-source">
                        {item.source === "bundled" ? "встроенный" : "пользовательский"}
                      </span>
                    </span>
                  </label>
                  {hasDetail ? (
                    <button
                      type="button"
                      className="skills-expand-btn"
                      aria-expanded={isOpen}
                      aria-label={isOpen ? "Скрыть описание" : "Показать описание"}
                      onClick={() => toggleExpanded(key)}
                    >
                      <Chevron size={14} aria-hidden />
                    </button>
                  ) : null}
                  <button
                    type="button"
                    className="skills-icon-btn"
                    aria-label={`Просмотреть скил ${item.name}`}
                    title="Просмотреть содержимое"
                    onClick={() => setPreviewed(item)}
                  >
                    <Eye size={14} aria-hidden />
                  </button>
                  {item.source === "user" ? (
                    <button
                      type="button"
                      className="settings-link-btn danger"
                      disabled={busy}
                      onClick={() => void removeSkill(item)}
                    >
                      Удалить
                    </button>
                  ) : null}
                </div>
                {hasDetail && isOpen ? (
                  <p className={`skills-description${invalid ? " is-error" : ""}`}>{detail}</p>
                ) : null}
              </div>
            );
          })
        )}
      </div>
      {error ? <div className="settings-error">{error}</div> : null}
      {previewed ? (
        <SkillPreviewModal skill={previewed} onClose={() => setPreviewed(null)} />
      ) : null}
    </>
  );
}
