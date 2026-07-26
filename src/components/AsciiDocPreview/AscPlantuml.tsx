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
const FIT_PADDING = 32; // px around the diagram when "fit to container"

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

  // Zoom/pan state. `scale` is the CSS scale applied to the SVG wrapper;
  // `tx`/`ty` are translate offsets in px (relative to the viewport origin).
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [state, setState] = useState<RenderState>({ kind: "loading" });

  const viewportRef = useRef<HTMLDivElement | null>(null);
  const wrapperRef = useRef<HTMLDivElement | null>(null);

  // Drag state kept in refs — pointermove fires a lot and we don't want
  // a React state round-trip per pixel.
  const dragState = useRef<{
    active: boolean;
    startX: number;
    startY: number;
    baseX: number;
    baseY: number;
  }>({ active: false, startX: 0, startY: 0, baseX: 0, baseY: 0 });

  // Reset zoom/pan whenever a new diagram source comes in.
  useEffect(() => {
    setScale(1);
    setOffset({ x: 0, y: 0 });
  }, [rawSource]);

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
    setOffset({ x: 0, y: 0 });
  }, []);

  /** Scale the diagram so it fits the viewport width (with padding). */
  const fitToContainer = useCallback(() => {
    const viewport = viewportRef.current;
    const wrapper = wrapperRef.current;
    if (!viewport || !wrapper) {
      resetView();
      return;
    }
    const svg = wrapper.querySelector("svg");
    if (!svg) {
      resetView();
      return;
    }
    const naturalW = svg.getBoundingClientRect().width / scale;
    const naturalH = svg.getBoundingClientRect().height / scale;
    const availW = viewport.clientWidth - FIT_PADDING * 2;
    const availH = viewport.clientHeight - FIT_PADDING * 2;
    const s = Math.min(availW / naturalW, availH / naturalH, 1);
    setScale(clampScale(s));
    setOffset({ x: 0, y: 0 });
  }, [scale, clampScale, resetView]);

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
        baseX: offset.x,
        baseY: offset.y,
      };
    },
    [offset],
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      const ds = dragState.current;
      if (!ds.active) return;
      const dx = e.clientX - ds.startX;
      const dy = e.clientY - ds.startY;
      setOffset({ x: ds.baseX + dx, y: ds.baseY + dy });
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
            <div
              ref={wrapperRef}
              className="asc-plantuml-svg"
              style={{
                transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale})`,
              }}
              dangerouslySetInnerHTML={{ __html: state.svg }}
            />
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
