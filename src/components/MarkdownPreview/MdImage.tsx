import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { toMessage } from "../../lib/errors";
import { resolveAssetTargetDocsRelative } from "../../lib/paths";
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
  filePath = null,
}: {
  src: string | undefined;
  alt: string | undefined;
  docsRoot: string | null;
  /** Docs-relative path of the markdown file (for `../` targets). */
  filePath?: string | null;
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

    const primary = resolveAssetTargetDocsRelative(target, filePath);
    const raw = target.replace(/\\/g, "/").replace(/^\/+/, "");
    const candidates = primary === raw ? [primary] : [primary, raw];

    let cancelled = false;
    setState({ kind: "loading" });

    void (async () => {
      let lastError: unknown = null;
      for (const candidate of candidates) {
        try {
          const canonical = await resolveAssetPath(docsRoot, candidate);
          if (cancelled) return;
          setState({ kind: "loaded", src: convertFileSrc(canonical) });
          return;
        } catch (e) {
          lastError = e;
        }
      }
      if (cancelled) return;
      setState({
        kind: "error",
        message:
          toMessage(lastError),
      });
    })();

    return () => {
      cancelled = true;
    };
  }, [target, docsRoot, filePath]);

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
