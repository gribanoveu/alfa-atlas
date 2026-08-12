import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { resolveAssetPath } from "../../lib/project";
import "./ImagePreview.css";

type LoadState =
  | { kind: "loading" }
  | { kind: "loaded"; src: string }
  | { kind: "error"; message: string };

/**
 * Full-tab preview for image assets opened from the docs tree.
 * Resolves via `resolve_asset_path` + `convertFileSrc` (same stack as AscImage).
 */
export function ImagePreview({
  relativePath,
  docsRoot,
}: {
  relativePath: string;
  docsRoot: string | null;
}) {
  const [state, setState] = useState<LoadState>({ kind: "loading" });
  const name = relativePath.split(/[/\\]/).pop() ?? relativePath;

  useEffect(() => {
    if (!docsRoot) {
      setState({ kind: "error", message: "docsRoot unknown" });
      return;
    }
    let cancelled = false;
    setState({ kind: "loading" });
    resolveAssetPath(docsRoot, relativePath)
      .then((canonical) => {
        if (cancelled) return;
        setState({ kind: "loaded", src: convertFileSrc(canonical) });
      })
      .catch((e: unknown) => {
        if (cancelled) return;
        setState({
          kind: "error",
          message: e instanceof Error ? e.message : String(e),
        });
      });
    return () => {
      cancelled = true;
    };
  }, [docsRoot, relativePath]);

  return (
    <div className="image-preview">
      {state.kind === "loaded" ? (
        <img className="image-preview-img" src={state.src} alt={name} />
      ) : (
        <div
          className={`image-preview-placeholder ${
            state.kind === "error" ? "image-preview-placeholder-error" : ""
          }`}
        >
          {state.kind === "error" ? state.message : "Загрузка…"}
        </div>
      )}
      <div className="image-preview-caption">{relativePath}</div>
    </div>
  );
}
