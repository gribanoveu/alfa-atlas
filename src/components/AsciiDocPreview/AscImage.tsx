import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { resolveAssetPath } from "../../lib/project";
import type { AbstractBlock } from "./types";

type LoadState =
  | { kind: "loading" }
  | { kind: "loaded"; src: string }
  | { kind: "error"; message: string };

const EXTERNAL_RE = /^https?:\/\//i;

/**
 * Блок изображения `image::target[alt]`.
 *
 * Локальные пути резолвятся против docsRoot через backend-команду
 * `resolve_asset_path` (валидация `..` и containment), затем превращаются
 * в WebView-loadable URL через `convertFileSrc`. Внешние `http(s)://`
 * и `data:` URL отдаются напрямую.
 */
export function AscImage({
  block,
  docsRoot,
}: {
  block: AbstractBlock;
  docsRoot: string | null;
}) {
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
