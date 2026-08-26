import { useSkills } from "../../hooks/useSkills";
import "../Welcome/CloneRepoModal.css";
import "./SkillsTab.css";

export function SkillsTab() {
  const { items, error, busy, toggle, addSkill, removeSkill, openFolder } = useSkills();

  return (
    <>
      <div className="settings-section-title">Скилы ассистента</div>
      <p className="settings-lead">
        Специализированные инструкции в формате Agent Skills. Ассистент ищет и
        загружает их через тул skill, полный список в промпт не попадает.
        Выключенный скил не находится поиском.
      </p>
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
            return (
              <div
                key={`${item.source}:${item.name}`}
                className={`skills-row${invalid ? " is-invalid" : ""}`}
              >
                <label className="settings-check">
                  <input
                    type="checkbox"
                    checked={item.enabled && !invalid}
                    disabled={busy || invalid}
                    onChange={(event) => void toggle(item, event.target.checked)}
                  />
                  <span className="skills-row-body">
                    <span className="skills-row-title">
                      <span className="skills-name">{item.name}</span>
                      <span className="skills-source">
                        {item.source === "bundled" ? "встроенный" : "пользовательский"}
                      </span>
                    </span>
                    <span className="skills-description">
                      {invalid ? item.error : item.description}
                    </span>
                  </span>
                </label>
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
            );
          })
        )}
      </div>
      {error ? <div className="settings-error">{error}</div> : null}
    </>
  );
}
