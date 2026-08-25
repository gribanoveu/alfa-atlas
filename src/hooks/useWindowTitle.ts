import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";

const APP_NAME = "Alfa Atlas";

export function formatWindowTitle(projectName: string | null | undefined): string {
  const trimmed = projectName?.trim();
  return trimmed ? `${trimmed} — ${APP_NAME}` : APP_NAME;
}

export function useWindowTitle(projectName: string | null | undefined): void {
  useEffect(() => {
    const title = formatWindowTitle(projectName);
    document.title = title;
    void getCurrentWindow().setTitle(title);
  }, [projectName]);
}
