import type { AbstractBlock } from "./types";

/**
 * Блок изображения `image::target[alt]`.
 *
 * На первом этапе показывается alt-текст как плейсхолдер. Ресолвинг пути
 * против docsRoot и подгрузка бинарного ассета через Tauri asset-protocol
 * — follow-up (требует `core:asset` capability и проверки путей).
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

  const src = resolveImageSrc(target, docsRoot);

  return (
    <figure className="asc-image">
      {src ? (
        <img src={src} alt={alt} />
      ) : (
        <div className="asc-image-placeholder" title={target ?? undefined}>
          <span className="asc-image-placeholder-icon">[image]</span>
          <span className="asc-image-placeholder-alt">{alt}</span>
        </div>
      )}
      {block.title ? (
        <figcaption className="asc-image-caption">{block.title}</figcaption>
      ) : null}
    </figure>
  );
}

function resolveImageSrc(
  target: string | null,
  _docsRoot: string | null,
): string | null {
  if (!target) return null;
  // Внешние URL и data: — отдаём как есть.
  if (/^https?:\/\//i.test(target) || target.startsWith("data:")) {
    return target;
  }
  // Локальные изображения требуют asset-protocol — пока не поддерживается.
  return null;
}
