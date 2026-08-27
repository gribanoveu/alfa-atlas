import { useCallback, useEffect, useRef, useState } from "react";
import type { GitDiffScope, GitFileDiff, GitFileStatus } from "../lib/git";

type Deps = {
  target: { file: GitFileStatus; scope: GitDiffScope };
  onLoadDiff: (path: string, scope: GitDiffScope) => Promise<GitFileDiff | null>;
  onDiscard: (path: string) => Promise<boolean>;
  onSaveContent: (path: string, scope: GitDiffScope, content: string) => Promise<boolean>;
};

/** One file's diff, and the two things the diff view can do to it.
 *
 * Reloads whenever the target changes, and again after a save — otherwise
 * the view would keep showing the pre-save diff, with hunks the user just
 * reverted still displayed as changes. */
export function useGitFileDiff({ target, onLoadDiff, onDiscard, onSaveContent }: Deps) {
  const [diff, setDiff] = useState<GitFileDiff | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [discarding, setDiscarding] = useState(false);
  const [saving, setSaving] = useState(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const path = target.file.path;
  const scope = target.scope;

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setDiff(null);

    void onLoadDiff(path, scope).then((result) => {
      if (cancelled) return;
      if (!result) setError("Не удалось загрузить diff");
      else setDiff(result);
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [onLoadDiff, path, scope]);

  /** `true` when the changes were dropped, so the caller can close. */
  const discard = useCallback(async () => {
    setDiscarding(true);
    setError(null);
    try {
      const ok = await onDiscard(path);
      if (!ok && mounted.current) setError("Не удалось отменить изменения");
      return ok;
    } finally {
      if (mounted.current) setDiscarding(false);
    }
  }, [onDiscard, path]);

  /** `"saved"` — written and the view refreshed; `"gone"` — written but the
   * file no longer has a diff, so the caller should close; `"failed"` — the
   * write did not land and the error is set. */
  const save = useCallback(
    async (content: string): Promise<"saved" | "gone" | "failed"> => {
      setSaving(true);
      setError(null);
      try {
        const ok = await onSaveContent(path, scope, content);
        if (!ok) {
          if (mounted.current) setError("Не удалось сохранить изменения");
          return "failed";
        }
        const result = await onLoadDiff(path, scope);
        if (!mounted.current) return "saved";
        if (!result) return "gone";
        setDiff(result);
        return "saved";
      } finally {
        if (mounted.current) setSaving(false);
      }
    },
    [onLoadDiff, onSaveContent, path, scope],
  );

  return { diff, loading, error, discarding, saving, discard, save };
}
