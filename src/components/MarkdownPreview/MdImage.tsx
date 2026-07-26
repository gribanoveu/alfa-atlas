import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { resolveAssetPath } from "../../lib/project";

type LoadState =
  | { kind: "loading" }
  | { kind: "loaded"; src: string }
  | { kind: "error"; message: string };

const EXTERNAL_RE = /^https?:\/\//i;

/** Markdown image with local path resolution against docsRoot. */
export function MdImage({
  src,
  alt,
  docsRoot,
}: {
  src: string | undefined;
  alt: string | undefined;
  docsRoot: string | null;
}) {
  const target = src ?? "";
  const label = alt ?? target ?? "image";

  const [state, setState] = useState<LoadState>({ kind: "loading" });

  useEffect(() => {
    if (!target) {
      setState({ kind: "error", message: "no src" });
      return;
    }
    if (EXTERNAL_RE.test(target) || target.startsWith("data:")) {
      setState({ kind: "loaded", src: target });
      return;
    }
    if (!docsRoot) {
      setState({ kind: "error", message: "docsRoot unknown" });
      return;
    }

    let cancelled = false;
    setState({ kind: "loading" });
    resolveAssetPath(docsRoot, target)
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
  }, [target, docsRoot]);

  return (
    <figure className="asc-image">
      {state.kind === "loaded" ? (
        <img src={state.src} alt={label} />
      ) : (
        <div
          className={`asc-image-placeholder ${
            state.kind === "error" ? "asc-image-placeholder-error" : ""
          }`}
          title={target || undefined}
        >
          <span className="asc-image-placeholder-icon">
            {state.kind === "error" ? "[image error]" : "[image]"}
          </span>
          <span className="asc-image-placeholder-alt">{label}</span>
        </div>
      )}
    </figure>
  );
}
