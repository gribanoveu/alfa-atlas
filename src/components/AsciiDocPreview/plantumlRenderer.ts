/**
 * PlantUML rendering engine — thin wrapper around the vendored TeaVM-compiled
 * `@plantuml/core` engine in `src/vendor/plantuml/`.
 *
 * The engine is loaded lazily on the first `renderPlantuml()` call and shared
 * across all components. `renderToString` uses shared internal state, so
 * renders MUST be serialized — `processQueue()` runs them one at a time.
 *
 * Files are vendored (not pulled from CDN at runtime) per project requirement:
 * no external network requests are made to render diagrams.
 */

type RenderResult = { kind: "ok"; svg: string } | { kind: "error"; message: string };

interface PlantumlEngine {
  renderToString(
    lines: string[],
    onSuccess: (svg: string) => void,
    onError: (err: string) => void,
  ): void;
}

// `viz-global.js` must be loaded as a classic <script> before the engine
// module is imported. Vite's `?url` suffix gives us a bundleable asset URL.
import vizGlobalUrl from "../../vendor/plantuml/viz-global.js?url";

let enginePromise: Promise<PlantumlEngine> | null = null;
let vizScriptLoaded: Promise<void> | null = null;

const RENDER_TIMEOUT_MS = 15_000;

/** Track the global so we don't inject the script twice across HMR. */
declare global {
  interface Window {
    __plantumlVizLoaded?: boolean;
  }
}

function loadVizGlobal(): Promise<void> {
  if (vizScriptLoaded) return vizScriptLoaded;
  if (window.__plantumlVizLoaded) {
    vizScriptLoaded = Promise.resolve();
    return vizScriptLoaded;
  }
  vizScriptLoaded = new Promise<void>((resolve, reject) => {
    const script = document.createElement("script");
    script.src = vizGlobalUrl;
    script.async = false;
    script.onload = () => {
      window.__plantumlVizLoaded = true;
      resolve();
    };
    script.onerror = () => {
      vizScriptLoaded = null;
      reject(new Error("Failed to load viz-global.js"));
    };
    document.head.appendChild(script);
  });
  return vizScriptLoaded;
}

async function loadEngine(): Promise<PlantumlEngine> {
  if (enginePromise) return enginePromise;
  enginePromise = (async () => {
    await loadVizGlobal();
    // Dynamic import → Vite code-splits the ~7MB engine into a separate chunk
    // loaded only when a plantuml block is actually rendered.
    const mod = await import("../../vendor/plantuml/plantuml.js");
    return mod as unknown as PlantumlEngine;
  })();
  return enginePromise;
}

type QueueJob = () => Promise<void>;

const queue: QueueJob[] = [];
let processing = false;

function processQueue(): void {
  if (processing || queue.length === 0) return;
  processing = true;
  const job = queue.shift()!;
  job().finally(() => {
    processing = false;
    processQueue();
  });
}

/**
 * Render a PlantUML source string to an SVG string.
 *
 * Source should include `@startuml` / `@enduml` markers (PlantUML convention).
 * The promise resolves once the engine has produced the SVG; multiple calls
 * are serialized internally because the engine uses shared state.
 */
export function renderPlantuml(source: string): Promise<RenderResult> {
  return new Promise<RenderResult>((resolve) => {
    const run = async () => {
      try {
        const engine = await loadEngine();
        const lines = source.split(/\r\n|\r|\n/);

        let settled = false;
        const timer = window.setTimeout(() => {
          if (settled) return;
          settled = true;
          resolve({
            kind: "error",
            message: `PlantUML render timed out after ${RENDER_TIMEOUT_MS / 1000}s`,
          });
        }, RENDER_TIMEOUT_MS);

        engine.renderToString(
          lines,
          (svg) => {
            if (settled) return;
            settled = true;
            window.clearTimeout(timer);
            resolve({ kind: "ok", svg });
          },
          (err) => {
            if (settled) return;
            settled = true;
            window.clearTimeout(timer);
            resolve({ kind: "error", message: err || "PlantUML render error" });
          },
        );
      } catch (e) {
        resolve({
          kind: "error",
          message: e instanceof Error ? e.message : String(e),
        });
      }
    };

    queue.push(run);
    processQueue();
  });
}
