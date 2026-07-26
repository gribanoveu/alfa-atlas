import { useCallback, useEffect, useRef, useState } from "react";
import { readProjectFile } from "../../lib/project";
import type { AbstractBlock } from "./types";
import { renderPlantuml } from "./plantumlRenderer";

type RenderState =
  | { kind: "loading" }
  | { kind: "ok"; svg: string }
  | { kind: "error"; message: string };

const MIN_SCALE = 0.2;
const MAX_SCALE = 5;
const SCALE_STEP = 0.2;
const VIEWPORT_PADDING = 16;

type SvgSize = { w: number; h: number };

/** Natural pixel size of a PlantUML SVG (width/height attrs or viewBox). */
function measureSvgSize(svg: SVGSVGElement): SvgSize {
  const widthAttr = svg.getAttribute("width");
  const heightAttr = svg.getAttribute("height");
  if (widthAttr && heightAttr) {
    const w = parseFloat(widthAttr);
    const h = parseFloat(heightAttr);
    if (!Number.isNaN(w) && !Number.isNaN(h) && w > 0 && h > 0) {
      return { w, h };
    }
  }
  const vb = svg.getAttribute("viewBox");
  if (vb) {
    const parts = vb.split(/[\s,]+/).map(Number);
    if (parts.length === 4 && parts.every((n) => !Number.isNaN(n))) {
      return { w: parts[2], h: parts[3] };
    }
  }
  const rect = svg.getBoundingClientRect();
  return { w: rect.width || 1, h: rect.height || 1 };
}

/**
 * `include::target.puml[]` inside a listing block. asciidoctor does NOT apply
 * include-substitution to listing blocks by default, so `[plantuml] ----
 * include::foo.puml[] ----` reaches us with empty `getSource()`. We detect
 * such directives and resolve them via the `read_project_file` Tauri command
 * (same path validation as the IncludeProcessor in `useAsciiDocRender`).
 *
 * Multiple includes are allowed; unresolved ones are left in place so the
 * PlantUML engine surfaces a clear error rather than silently empty input.
 */
const INCLUDE_RE = /^include::([^\[\]]+)\[(.*)\]\s*$/;

/** Drop leading/trailing blank lines; asciidoctor listing blocks often add them. */
function normalizePlantumlSource(raw: string): string {
  const lines = raw.split(/\r\n|\r|\n/);
  while (lines.length > 0 && lines[0].trim() === "") lines.shift();
  while (lines.length > 0 && lines[lines.length - 1].trim() === "") lines.pop();
  return lines.join("\n");
}

async function expandIncludes(
  raw: string,
  docsRoot: string | null,
): Promise<string> {
  if (!raw || !docsRoot) return raw;
  const outLines: string[] = [];
  for (const line of raw.split(/\r\n|\r|\n/)) {
    const m = INCLUDE_RE.exec(line);
    if (!m) {
      outLines.push(line);
      continue;
    }
    const target = m[1];
    try {
      const content = await readProjectFile(docsRoot, target);
      outLines.push(content);
    } catch {
      // Leave the directive in place — the engine will report it as unknown,
      // which is more informative than a silently empty diagram.
      outLines.push(line);
    }
  }
  return outLines.join("\n");
}

/**
 * PlantUML-диаграмма: блок `[plantuml,…] ---- … ----` (или standalone
 * `.puml`-файл — см. `AsciiDocPreview`).
 *
 * asciidoctor раскрывает `include::file.puml[]` через IncludeProcessor в
 * `useAsciiDocRender` (с учётом `filePath` документа). `expandIncludes` ниже —
 * запасной путь, если директива осталась в `getSource()`. Рендер идёт через
 * vendored TeaVM-движок (`plantumlRenderer.ts`) — без сервера и Java.
 *
 * Просмотр диаграммы:
 *  - zoom in / out / reset / fit кнопками или колесом мыши (Ctrl+колесо).
 *  - pan: drag левой кнопкой мыши по холсту (cursor: grabbing в активном
 *    состоянии). Скролл-контейнер остаётся, когда диаграмма больше холста.
 *
 * При ошибке показываем сообщение и исходный текст (fallback к listing-виду).
 */
export function AscPlantuml({
  block,
  docsRoot = null,
}: {
  block: AbstractBlock;
  docsRoot?: string | null;
}) {
  const rawSource = safeGetSource(block) ?? "";

  const [scale, setScale] = useState(1);
  const [naturalSize, setNaturalSize] = useState<SvgSize | null>(null);
  const [state, setState] = useState<RenderState>({ kind: "loading" });

  const viewportRef = useRef<HTMLDivElement | null>(null);
  const wrapperRef = useRef<HTMLDivElement | null>(null);

  const dragState = useRef<{
    active: boolean;
    startX: number;
    startY: number;
    baseScrollLeft: number;
    baseScrollTop: number;
  }>({ active: false, startX: 0, startY: 0, baseScrollLeft: 0, baseScrollTop: 0 });

  // Reset zoom whenever a new diagram source comes in.
  useEffect(() => {
    setScale(1);
    setNaturalSize(null);
  }, [rawSource]);

  useEffect(() => {
    if (state.kind !== "ok") {
      setNaturalSize(null);
      return;
    }
    const frame = requestAnimationFrame(() => {
      const svg = wrapperRef.current?.querySelector("svg");
      if (svg) setNaturalSize(measureSvgSize(svg));
    });
    return () => cancelAnimationFrame(frame);
  }, [state]);

  useEffect(() => {
    let cancelled = false;
    setState({ kind: "loading" });
    expandIncludes(rawSource, docsRoot)
      .then((source) => renderPlantuml(normalizePlantumlSource(source)))
      .then((r) => {
        if (cancelled) return;
        if (r.kind === "ok") {
          setState({ kind: "ok", svg: r.svg });
        } else {
          setState({ kind: "error", message: r.message });
        }
      })
      .catch(() => {
        if (cancelled) return;
        setState({ kind: "error", message: "render failed" });
      });
    return () => {
      cancelled = true;
    };
  }, [rawSource, docsRoot]);

  const clampScale = useCallback((s: number) => {
    return Math.min(MAX_SCALE, Math.max(MIN_SCALE, s));
  }, []);

  const zoomIn = useCallback(() => {
    setScale((s) => clampScale(s + SCALE_STEP));
  }, [clampScale]);

  const zoomOut = useCallback(() => {
    setScale((s) => clampScale(s - SCALE_STEP));
  }, [clampScale]);

  const resetView = useCallback(() => {
    setScale(1);
    const viewport = viewportRef.current;
    if (viewport) {
      viewport.scrollLeft = 0;
      viewport.scrollTop = 0;
    }
  }, []);

  /** Scale the diagram to fit the viewport; scroll area matches visual size. */
  const fitToContainer = useCallback(() => {
    const viewport = viewportRef.current;
    if (!viewport || !naturalSize) {
      resetView();
      return;
    }
    const pad = VIEWPORT_PADDING;
    const availW = Math.max(1, viewport.clientWidth - pad * 2);
    const availH = Math.max(1, viewport.clientHeight - pad * 2);
    const s = clampScale(
      Math.min(availW / naturalSize.w, availH / naturalSize.h),
    );
    setScale(s);
    requestAnimationFrame(() => {
      const sw = naturalSize.w * s;
      const sh = naturalSize.h * s;
      viewport.scrollLeft = Math.max(0, (sw + pad * 2 - viewport.clientWidth) / 2);
      viewport.scrollTop = Math.max(0, (sh + pad * 2 - viewport.clientHeight) / 2);
    });
  }, [naturalSize, clampScale, resetView]);

  /** Ctrl+wheel zooms toward the cursor; plain wheel scrolls. */
  const handleWheel = useCallback(
    (e: React.WheelEvent<HTMLDivElement>) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      const delta = e.deltaY > 0 ? -SCALE_STEP : SCALE_STEP;
      setScale((s) => clampScale(s + delta));
    },
    [clampScale],
  );

  const handlePointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (e.button !== 0) return;
      const target = e.target as HTMLElement;
      if (target.closest("button")) return;
      const viewport = viewportRef.current;
      if (!viewport) return;
      viewport.setPointerCapture(e.pointerId);
      dragState.current = {
        active: true,
        startX: e.clientX,
        startY: e.clientY,
        baseScrollLeft: viewport.scrollLeft,
        baseScrollTop: viewport.scrollTop,
      };
    },
    [],
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      const ds = dragState.current;
      const viewport = viewportRef.current;
      if (!ds.active || !viewport) return;
      viewport.scrollLeft = ds.baseScrollLeft - (e.clientX - ds.startX);
      viewport.scrollTop = ds.baseScrollTop - (e.clientY - ds.startY);
    },
    [],
  );

  const endDrag = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      const ds = dragState.current;
      if (!ds.active) return;
      const viewport = viewportRef.current;
      if (viewport && viewport.hasPointerCapture(e.pointerId)) {
        viewport.releasePointerCapture(e.pointerId);
      }
      dragState.current = { ...ds, active: false };
    },
    [],
  );

  const name = block.getAttribute("1") as string | null;
  const isPanning = dragState.current.active;
  const zoomLabel = `${Math.round(scale * 100)}%`;
  const scaledW = naturalSize ? naturalSize.w * scale : undefined;
  const scaledH = naturalSize ? naturalSize.h * scale : undefined;

  return (
    <div className="asc-plantuml" data-name={name ?? undefined}>
      {state.kind === "loading" ? (
        <div className="asc-plantuml-loading">Рендеринг диаграммы…</div>
      ) : state.kind === "error" ? (
        <div className="asc-plantuml-error">
          <div className="asc-plantuml-error-title">Ошибка PlantUML</div>
          <pre className="asc-plantuml-error-message">{state.message}</pre>
          <pre className="asc-plantuml-source">{rawSource}</pre>
        </div>
      ) : (
        <>
          <div className="asc-plantuml-toolbar">
            <button
              type="button"
              className="asc-plantuml-btn"
              onClick={zoomOut}
              disabled={scale <= MIN_SCALE + 1e-6}
              title="Уменьшить"
              aria-label="Уменьшить масштаб"
            >
              −
            </button>
            <span className="asc-plantuml-zoom" aria-live="polite">
              {zoomLabel}
            </span>
            <button
              type="button"
              className="asc-plantuml-btn"
              onClick={zoomIn}
              disabled={scale >= MAX_SCALE - 1e-6}
              title="Увеличить"
              aria-label="Увеличить масштаб"
            >
              +
            </button>
            <button
              type="button"
              className="asc-plantuml-btn"
              onClick={fitToContainer}
              title="Вписать в контейнер"
              aria-label="Вписать в контейнер"
            >
              Fit
            </button>
            <button
              type="button"
              className="asc-plantuml-btn"
              onClick={resetView}
              title="Сбросить масштаб (100%)"
              aria-label="Сбросить масштаб"
            >
              1:1
            </button>
          </div>
          <div
            ref={viewportRef}
            className={`asc-plantuml-viewport${isPanning ? " is-panning" : ""}`}
            onWheel={handleWheel}
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={endDrag}
            onPointerCancel={endDrag}
          >
            <div className="asc-plantuml-canvas">
              <div
                ref={wrapperRef}
                className="asc-plantuml-svg"
                style={
                  scaledW && scaledH
                    ? { width: scaledW, height: scaledH }
                    : undefined
                }
                dangerouslySetInnerHTML={{ __html: state.svg }}
              />
            </div>
          </div>
        </>
      )}
    </div>
  );
}

function safeGetSource(block: AbstractBlock): string | null {
  const fn = (block as unknown as { getSource?: () => string }).getSource;
  return typeof fn === "function" ? fn.call(block) : null;
}
