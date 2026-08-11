import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { isSupportedFile } from "../lib/supportedFiles";

export type OsFileDropHandlers = {
  /** Import OS files into a docs-tree directory (relative path, `"."` = root). */
  onImportExternal: (destDirPath: string, absolutePaths: string[]) => void;
  /** Open a supported text file from an absolute OS path (no copy). */
  onOpenExternal: (absolutePath: string) => void;
  /** Rejected drop (unsupported format outside a folder). */
  onReject: (message: string) => void;
};

function dropDirAtPoint(x: number, y: number): string | null {
  const el = document.elementFromPoint(x, y);
  if (!el || !(el instanceof Element)) return null;
  const host = el.closest("[data-drop-dir]");
  if (!host) return null;
  return host.getAttribute("data-drop-dir");
}

/**
 * Map Tauri drag-drop `PhysicalPosition` into CSS viewport coords for
 * `elementFromPoint`.
 *
 * On macOS (and Linux), wry reports view/widget points that already match
 * CSS pixels, but Tauri still types them as `PhysicalPosition`. Dividing by
 * `devicePixelRatio` shifts the hit-test above the cursor on Retina.
 *
 * On Windows, WebView2/`ScreenToClient` returns real physical pixels, so we
 * must divide by DPR.
 */
function toCssPoint(x: number, y: number): { x: number; y: number } {
  const isWindows =
    typeof navigator !== "undefined" &&
    /win/i.test(navigator.platform || navigator.userAgent);
  if (!isWindows) {
    return { x, y };
  }
  const dpr = window.devicePixelRatio || 1;
  return { x: x / dpr, y: y / dpr };
}

function fileNameOf(absolutePath: string): string {
  return absolutePath.split(/[/\\]/).pop() ?? absolutePath;
}

/**
 * Listen for OS file drops on the Tauri webview. Hit-tests `[data-drop-dir]`
 * under the cursor so drops land on a docs folder or open as external text.
 */
export function useOsFileDrop(
  enabled: boolean,
  handlers: OsFileDropHandlers,
): { osDropTargetPath: string | null } {
  const [osDropTargetPath, setOsDropTargetPath] = useState<string | null>(null);
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;

  useEffect(() => {
    if (!enabled) {
      setOsDropTargetPath(null);
      return;
    }

    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (cancelled) return;
        const { payload } = event;

        if (payload.type === "leave") {
          setOsDropTargetPath(null);
          return;
        }

        if (payload.type === "enter" || payload.type === "over") {
          const { x, y } = toCssPoint(payload.position.x, payload.position.y);
          setOsDropTargetPath(dropDirAtPoint(x, y));
          return;
        }

        if (payload.type === "drop") {
          const { x, y } = toCssPoint(payload.position.x, payload.position.y);
          const destDir = dropDirAtPoint(x, y);
          setOsDropTargetPath(null);

          const paths = payload.paths;
          if (paths.length === 0) return;

          if (destDir !== null) {
            handlersRef.current.onImportExternal(destDir, paths);
            return;
          }

          for (const absolutePath of paths) {
            if (isSupportedFile(absolutePath)) {
              handlersRef.current.onOpenExternal(absolutePath);
            } else {
              handlersRef.current.onReject(
                `Перетащите «${fileNameOf(absolutePath)}» в папку проводника — этот формат нельзя открыть как временный файл`,
              );
            }
          }
        }
      })
      .then((fn) => {
        if (cancelled) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => {
        // Not running under Tauri (e.g. vite-only) — ignore.
      });

    return () => {
      cancelled = true;
      unlisten?.();
      setOsDropTargetPath(null);
    };
  }, [enabled]);

  return { osDropTargetPath };
}
