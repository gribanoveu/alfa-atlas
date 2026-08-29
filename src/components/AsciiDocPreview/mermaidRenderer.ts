/**
 * Mermaid rendering engine — thin wrapper around the `mermaid` npm package.
 *
 * The library is loaded lazily on the first `renderMermaid()` call.
 * Renders are serialized because `mermaid.render()` uses shared internal state.
 */

import { toMessage } from "../../lib/errors";
import { DEFAULT_DIAGRAM_THEME, type DiagramTheme } from "../../lib/prefs";
type RenderResult = { kind: "ok"; svg: string } | { kind: "error"; message: string };

type MermaidModule = {
  initialize: (config: {
    startOnLoad: boolean;
    securityLevel: string;
    theme: string;
  }) => void;
  render: (id: string, text: string) => Promise<{ svg: string }>;
};

/** Our two palettes in Mermaid's own vocabulary.
 *
 * Deliberately Mermaid's *built-in* themes rather than `base` plus
 * `themeVariables` mapped from the app's tokens. The token-mapped version
 * looks better on flowcharts, but the categorical scales derive from
 * `primaryColor`/`secondaryColor`/`tertiaryColor` — feeding those the app's
 * greys collapses a pie chart into three near-identical slices, and fixing
 * that means hand-maintaining `pie1..pie12`, `cScale0..` and friends across
 * every diagram type. The built-in themes already cover all of them. */
const MERMAID_THEMES: Record<DiagramTheme, string> = {
  dark: "dark",
  light: "default",
};

let mermaidPromise: Promise<MermaidModule> | null = null;
let renderCounter = 0;

const RENDER_TIMEOUT_MS = 15_000;

/** Drop leading/trailing blank lines; asciidoctor listing blocks often add them. */
export function normalizeMermaidSource(raw: string): string {
  const lines = raw.split(/\r\n|\r|\n/);
  while (lines.length > 0 && lines[0].trim() === "") lines.shift();
  while (lines.length > 0 && lines[lines.length - 1].trim() === "") lines.pop();
  return lines.join("\n");
}

async function loadMermaid(): Promise<MermaidModule> {
  if (mermaidPromise) return mermaidPromise;
  mermaidPromise = (async () => {
    const mod = await import("mermaid");
    return (mod.default ?? mod) as MermaidModule;
  })();
  return mermaidPromise;
}

function nextRenderId(): string {
  renderCounter += 1;
  return `asc-mermaid-${renderCounter}-${crypto.randomUUID()}`;
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
 * Render a Mermaid source string to an SVG string.
 * Multiple calls are serialized internally.
 *
 * `theme` is applied per render rather than once at load: it is a live
 * setting, and `initialize` is the only way to change it. That is safe
 * precisely because of the queue above — no other render can be in flight
 * while this one re-configures the shared module.
 */
export function renderMermaid(
  source: string,
  theme: DiagramTheme = DEFAULT_DIAGRAM_THEME,
): Promise<RenderResult> {
  return new Promise<RenderResult>((resolve) => {
    const run = async () => {
      const normalized = normalizeMermaidSource(source);
      if (!normalized.trim()) {
        resolve({ kind: "error", message: "Mermaid diagram is empty" });
        return;
      }

      try {
        const mermaid = await loadMermaid();
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: MERMAID_THEMES[theme],
        });
        const id = nextRenderId();

        let settled = false;
        const timer = window.setTimeout(() => {
          if (settled) return;
          settled = true;
          resolve({
            kind: "error",
            message: `Mermaid render timed out after ${RENDER_TIMEOUT_MS / 1000}s`,
          });
        }, RENDER_TIMEOUT_MS);

        try {
          const { svg } = await mermaid.render(id, normalized);
          if (settled) return;
          settled = true;
          window.clearTimeout(timer);
          resolve({ kind: "ok", svg });
        } catch (e) {
          if (settled) return;
          settled = true;
          window.clearTimeout(timer);
          resolve({
            kind: "error",
            message: toMessage(e),
          });
        }
      } catch (e) {
        resolve({
          kind: "error",
          message: toMessage(e),
        });
      }
    };

    queue.push(run);
    processQueue();
  });
}
