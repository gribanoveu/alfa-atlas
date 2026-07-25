import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import {
  clearProjectRoot,
  getProjectRoot,
  setProjectRoot,
} from "../lib/project";

export function useProject() {
  const [projectRoot, setRoot] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const root = await getProjectRoot();
        if (!cancelled) {
          setRoot(root);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) {
          setRoot(null);
          setError(e instanceof Error ? e.message : String(e));
        }
      } finally {
        if (!cancelled) setReady(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const openProject = useCallback(async (path: string) => {
    const root = await setProjectRoot(path);
    setRoot(root);
    setError(null);
    return root;
  }, []);

  const openFolderDialog = useCallback(async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Открыть папку",
    });
    if (selected === null || Array.isArray(selected)) {
      return null;
    }
    return openProject(selected);
  }, [openProject]);

  const closeProject = useCallback(async () => {
    await clearProjectRoot();
    setRoot(null);
  }, []);

  const projectName = projectRoot
    ? projectRoot.split(/[/\\]/).filter(Boolean).pop() ?? projectRoot
    : null;

  return {
    projectRoot,
    projectName,
    ready,
    error,
    openProject,
    openFolderDialog,
    closeProject,
  };
}
