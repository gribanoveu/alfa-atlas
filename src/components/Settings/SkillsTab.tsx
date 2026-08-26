import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useState } from "react";
import {
  skillsImport,
  skillsList,
  skillsRemove,
  skillsSetEnabled,
  skillsUserDir,
  type SkillListItem,
} from "../../lib/skills";
import "../Welcome/CloneRepoModal.css";
import "./SkillsTab.css";
import { toMessage } from "../../lib/errors";

export function SkillsTab() {
  const [items, setItems] = useState<SkillListItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(async () => {
    const next = await skillsList();
    setItems(next);
    setError(null);
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const next = await skillsList();
        if (!cancelled) {
          setItems(next);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(toMessage(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const toggle = useCallback(
    async (item: SkillListItem, enabled: boolean) => {
      setBusy(true);
      try {
        await skillsSetEnabled(item.source, item.name, enabled);
        await reload();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [reload],
  );

  const addSkill = useCallback(async () => {
    const selected = await open({ directory: true, multiple: false, title: "Папка скила (SKILL.md)" });
    if (!selected || Array.isArray(selected)) return;
    setBusy(true);
    try {
      await skillsImport(selected);
      await reload();
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setBusy(false);
    }
  }, [reload]);

  const removeSkill = useCallback(
    async (item: SkillListItem) => {
      setBusy(true);
      try {
        await skillsRemove(item.name);
        await reload();
      } catch (e) {
        setError(toMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [reload],
  );

  const openFolder = useCallback(async () => {
    try {
      const dir = await skillsUserDir();
      await openPath(dir);
    } catch (e) {
      setError(toMessage(e));
    }
  }, []);

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
