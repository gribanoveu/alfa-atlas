import { useCallback, useEffect, useState } from "react";
import { toMessage } from "../lib/errors";
import { skillsFiles, skillsReadFile, type SkillSource } from "../lib/skills";

export type UseSkillPreview = {
  /** `null` until the file list resolves — the caller renders "Загрузка…". */
  files: string[] | null;
  /** The file being shown; `null` only while loading or for an empty skill. */
  selected: string | null;
  select: (path: string) => void;
  /** Text of `selected`, `null` while it loads or when the read failed. */
  content: string | null;
  loadingContent: boolean;
  error: string | null;
};

/** Read-only contents of one skill, for the Settings viewer. Loads the file
 * list once, then one file at a time — skills are small, but their companion
 * files need not all be in memory to show one of them. */
export function useSkillPreview(source: SkillSource, name: string): UseSkillPreview {
  const [files, setFiles] = useState<string[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState<string | null>(null);
  const [loadingContent, setLoadingContent] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setFiles(null);
    setSelected(null);
    setContent(null);
    setError(null);
    void skillsFiles(source, name)
      .then((next) => {
        if (cancelled) return;
        setFiles(next);
        // SKILL.md comes first from the backend, so this opens the skill
        // itself rather than whichever companion file sorts first.
        setSelected(next[0] ?? null);
      })
      .catch((e: unknown) => {
        if (cancelled) return;
        setFiles([]);
        setError(toMessage(e));
      });
    return () => {
      cancelled = true;
    };
  }, [source, name]);

  useEffect(() => {
    if (selected === null) {
      setContent(null);
      return;
    }
    let cancelled = false;
    setLoadingContent(true);
    setContent(null);
    void skillsReadFile(source, name, selected)
      .then((text) => {
        if (cancelled) return;
        setContent(text);
        setError(null);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(toMessage(e));
      })
      .finally(() => {
        if (!cancelled) setLoadingContent(false);
      });
    return () => {
      cancelled = true;
    };
  }, [source, name, selected]);

  const select = useCallback((path: string) => setSelected(path), []);

  return { files, selected, select, content, loadingContent, error };
}
