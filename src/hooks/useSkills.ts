import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import {
  skillsImport,
  skillsList,
  skillsRemove,
  skillsSetEnabled,
  skillsUserDir,
  type SkillListItem,
} from "../lib/skills";

export type UseSkills = {
  /** `null` until the first load resolves — the caller renders "Загрузка…". */
  items: SkillListItem[] | null;
  error: string | null;
  /** A mutating call is in flight; the caller disables its controls. */
  busy: boolean;
  toggle: (item: SkillListItem, enabled: boolean) => Promise<void>;
  /** Prompts for a folder, then imports it. A cancelled dialog is a no-op. */
  addSkill: () => Promise<void>;
  removeSkill: (item: SkillListItem) => Promise<void>;
  openFolder: () => Promise<void>;
};

/** The skills list and the four things the settings tab can do to it.
 *
 * Extracted from `SkillsTab.tsx`, which held this state and repeated the
 * same busy/try/catch shape at each call site — the smell the layering
 * skill calls out: a component doing its own fetching. */
export function useSkills(): UseSkills {
  const [items, setItems] = useState<SkillListItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const reload = useCallback(async () => {
    const next = await skillsList();
    if (!mounted.current) return;
    setItems(next);
    setError(null);
  }, []);

  /** Runs a mutating action, then refreshes the list. Holds `busy` for the
   * whole thing (not just the mutation) so the UI stays disabled until the
   * list it is about to re-render is actually current. */
  const run = useCallback(
    async (action: () => Promise<unknown>) => {
      setBusy(true);
      try {
        await action();
        await reload();
      } catch (e) {
        if (mounted.current) setError(toMessage(e));
      } finally {
        if (mounted.current) setBusy(false);
      }
    },
    [reload],
  );

  useEffect(() => {
    void reload().catch((e: unknown) => {
      if (mounted.current) setError(toMessage(e));
    });
  }, [reload]);

  const toggle = useCallback(
    (item: SkillListItem, enabled: boolean) =>
      run(() => skillsSetEnabled(item.source, item.name, enabled)),
    [run],
  );

  const removeSkill = useCallback(
    (item: SkillListItem) => run(() => skillsRemove(item.name)),
    [run],
  );

  const addSkill = useCallback(async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Папка скила (SKILL.md)",
    });
    // Cancelled dialog, or a multi-select we never asked for.
    if (!selected || Array.isArray(selected)) return;
    await run(() => skillsImport(selected));
  }, [run]);

  /** Reveals the user skills folder in the OS file manager. Not a mutation,
   * so it neither sets `busy` nor reloads. */
  const openFolder = useCallback(async () => {
    try {
      await openPath(await skillsUserDir());
    } catch (e) {
      if (mounted.current) setError(toMessage(e));
    }
  }, []);

  return { items, error, busy, toggle, addSkill, removeSkill, openFolder };
}
