import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { toMessage } from "../../lib/errors";
import { resolveAssetTargetDocsRelative } from "../../lib/paths";
import { resolveAssetPath } from "../../lib/project";
import type { AbstractBlock } from "./types";
import { useAscPreview } from "./AscPreviewContext";

type LoadState =
  | { kind: "loading" }
  | { kind: "loaded"; src: string }
  | { kind: "error"; message: string };

const EXTERNAL_RE = /^https?:\/\//i;

/**
 * Блок изображения `image::target[alt]`.
 *
 * Paths resolve like `include::` — relative to the previewed document's
 * directory — then via `resolve_asset_path` + `convertFileSrc`. If that
 * misses, fall back to treating the target as docs-root-relative (legacy
 * paths). Внешние `http(s)://` и `data:` URL отдаются напрямую.
 */
export function AscImage({
  block,
  docsRoot: docsRootProp,
}: {
  block: AbstractBlock;
  docsRoot?: string | null;
}) {
  const preview = useAscPreview();
  const docsRoot = docsRootProp ?? preview.docsRoot;
  const filePath = preview.filePath;
  const target = block.getAttribute("target") as string | null;
  const alt = (block.getAttribute("alt") as string | null) ?? target ?? "image";

  const [state, setState] = useState<LoadState>({ kind: "loading" });

  useEffect(() => {
    if (!target) {
      setState({ kind: "error", message: "no target" });
      return;
    }
    // Внешние URL и data: — синхронный passthrough, без backend-валидации.
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
        <img src={state.src} alt={alt} />
      ) : (
        <div
          className={`asc-image-placeholder ${
            state.kind === "error" ? "asc-image-placeholder-error" : ""
          }`}
          title={target ?? undefined}
        >
          <span className="asc-image-placeholder-icon">
            {state.kind === "error" ? "[image error]" : "[image]"}
          </span>
          <span className="asc-image-placeholder-alt">{alt}</span>
        </div>
      )}
      {block.title ? (
        <figcaption className="asc-image-caption">{block.title}</figcaption>
      ) : null}
    </figure>
  );
}
