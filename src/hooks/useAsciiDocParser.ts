import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import {
  type AsciiDocFacts,
  frontendReady,
  submitAsciiDocFacts,
} from "../lib/asciidocParser";

// asciidoctor v4 exposes top-level async `load` / `loadFile` and an
// `Extensions` registry as named exports — there is no default `Asciidoctor`
// constructor anymore. The package's `exports` field does not declare a
// `types` entry, so TypeScript resolves types through the shim in
// `src/types/asciidoctor.d.ts`.
import { Extensions, load, MemoryLogger, LoggerManager } from "asciidoctor";

type AsciiDocParseRequested = {
  documentId: string;
  version: number;
  content: string;
  relativePath: string;
};

type CapturedInclude = { path: string; line: number; column: number };

// Minimal shape of the IncludeProcessor DSL `this` context. The full
// `IncludeProcessorDslInterface` from asciidoctor's types is not resolvable
// through the package's `exports` field; we only need `handles` and `process`.
interface IncludeProcessorDsl {
  handles(fn: (target: string) => boolean): void;
  process(
    fn: (
      doc: unknown,
      reader: { lineno: number; pushInclude(data: string | string[]): unknown },
      target: string,
      attrs: Record<string, string>,
    ) => void,
  ): void;
}

/**
 * Build a fresh extension registry whose IncludeProcessor writes into
 * `captured`. Must be per-parse — a module-global buffer races when
 * `max_inflight > 1` concurrent `extractFacts` calls interleave.
 */
function createIncludeCapturingRegistry(captured: CapturedInclude[]) {
  const registry = Extensions.create();
  registry.includeProcessor(function (this: IncludeProcessorDsl) {
    this.handles(function (_target: string) {
      return true;
    });
    this.process(function (_doc, reader, target) {
      // `reader.lineno` points to the NEXT line to read (i.e. the line
      // after the include directive), so subtract 1 to get the directive's
      // own line. Floor at 1 for safety.
      const directiveLine = Math.max(1, reader.lineno - 1);
      captured.push({
        path: target,
        line: directiveLine,
        column: 1,
      });
      // Push empty content so the include is not expanded/resolved.
      reader.pushInclude([]);
    });
  });
  return registry;
}

/**
 * Extract AsciiDoc facts (anchors, includes, references, attributes, images,
 * parse errors) from `content` using `asciidoctor.js`.
 *
 * Exported for unit testing. Production callers go through `useAsciiDocParser`,
 * which wraps this in the IPC round-trip with full error recovery.
 *
 * Positions for inline constructs (anchors via `[#id]`, xrefs, attributes,
 * images) come from a line-scan of the raw content — asciidoctor's sourcemap
 * reports the source line of the enclosing block, not the directive line,
 * which would not match the old Rust parser's semantics. Block-level anchors
 * (`[[id]]` followed by a block) are also captured by the line scan for
 * consistency.
 */
export async function extractFacts(content: string): Promise<AsciiDocFacts> {
  const capturedIncludes: CapturedInclude[] = [];

  const facts: AsciiDocFacts = {
    anchors: [],
    includes: [],
    references: [],
    attributes: [],
    images: [],
    parseErrors: [],
  };

  // Capture parse-time warnings/errors via a MemoryLogger so genuine syntax
  // issues surface as ParseError diagnostics.
  const logger = new MemoryLogger();
  const previousLogger = LoggerManager.getLogger();
  LoggerManager.setLogger(logger);

  try {
    await load(content, {
      sourcemap: true,
      safe: "safe",
      attributes: { showtitle: true },
      // Per-parse registry — see `createIncludeCapturingRegistry`.
      extension_registry: createIncludeCapturingRegistry(capturedIncludes),
    });

    facts.includes = capturedIncludes.slice();
    facts.anchors = scanAnchors(content);
    facts.references = scanXrefs(content);
    facts.attributes = scanAttributes(content);
    facts.images = scanImages(content);

    for (const message of logger.getMessages()) {
      const loc = message.getSourceLocation();
      facts.parseErrors.push({
        message: message.getText(),
        line: loc ? loc.lineno : null,
        // asciidoctor labels table-layout quirks ("dropping cells…") as
        // ERROR even though the document still loads and facts extract
        // cleanly. Surface logger messages as warnings so they appear in
        // Problems without flipping the index status to "failed". Genuine
        // failures (thrown exceptions, IPC errors, timeouts) use "error".
        severity: "warning",
      });
    }
  } catch (e) {
    facts.parseErrors.push({
      message: e instanceof Error ? e.message : String(e),
      line: null,
      severity: "error",
    });
  } finally {
    LoggerManager.setLogger(previousLogger);
  }

  return facts;
}

// --- Regex helpers ---
//
// These scans mirror the line-based positions the old Rust parser produced.
// They are intentionally simple: positions only, no semantic resolution (the
// Rust coordinator owns cross-document semantics).

function scanAnchors(content: string): { id: string; line: number; column: number }[] {
  const out: { id: string; line: number; column: number }[] = [];
  const lines = content.split("\n");
  // Match `[[id]]` (block anchor) and `[#id]` (inline anchor on a block).
  // An `[[id]]` may also be written as `[[id,reftext]]` — capture just the id.
  const re = /\[\[([^\],]+)(?:,[^\]]*)?\]\]|\[#([^\]]+)\]/g;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    let m: RegExpExecArray | null;
    re.lastIndex = 0;
    while ((m = re.exec(line)) !== null) {
      const id = m[1] ?? m[2];
      if (id) {
        out.push({ id, line: i + 1, column: m.index + 1 });
      }
    }
  }
  return out;
}

function scanXrefs(content: string): {
  targetDocument: string;
  anchor: string | null;
  line: number;
  column: number;
}[] {
  const out: {
    targetDocument: string;
    anchor: string | null;
    line: number;
    column: number;
  }[] = [];
  const lines = content.split("\n");
  // Match `xref:target[#anchor][]` or `xref:target[]`. Target may be a path
  // (with optional `#fragment`) or just `#fragment` for same-doc anchors.
  const re = /xref:([^\[\]]+?)(?:#([^\[\]]+))?\[\]/g;
  // Angle-bracket short form: `<<target#anchor,text>>`, `<<target,text>>`,
  // `<<#anchor,text>>`, `<<target#anchor>>`, `<<target>>`, `<<#anchor>>`.
  // Target/anchor stop at `#`, `,` or `>`. Target is `*` (not `+`) so that a
  // leading `#anchor` form matches with an empty target.
  const shortRe = /<<([^,>#]*)(?:#([^,>#]+))?(?:,[^>]*)?>>/g;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    let m: RegExpExecArray | null;
    re.lastIndex = 0;
    while ((m = re.exec(line)) !== null) {
      const rawTarget = m[1];
      const anchor = m[2] ?? null;
      if (rawTarget.startsWith("#")) {
        out.push({
          targetDocument: "",
          anchor: rawTarget.slice(1),
          line: i + 1,
          column: m.index + 1,
        });
      } else {
        out.push({
          targetDocument: rawTarget,
          anchor,
          line: i + 1,
          column: m.index + 1,
        });
      }
    }
    shortRe.lastIndex = 0;
    while ((m = shortRe.exec(line)) !== null) {
      const rawTarget = m[1];
      const anchor = m[2] ?? null;
      if (!rawTarget || rawTarget.startsWith("#")) {
        out.push({
          targetDocument: "",
          anchor: rawTarget.startsWith("#") ? rawTarget.slice(1) : anchor,
          line: i + 1,
          column: m.index + 1,
        });
      } else {
        out.push({
          targetDocument: rawTarget,
          anchor,
          line: i + 1,
          column: m.index + 1,
        });
      }
    }
  }
  return out;
}

function scanAttributes(content: string): { name: string; value: string; line: number }[] {
  const out: { name: string; value: string; line: number }[] = [];
  const lines = content.split("\n");
  // Match `:name: value` (attribute entry). `!name!` (unset) is intentionally
  // ignored — it carries no value the index can use.
  const re = /^:(\w[\w-]*):\s*(.*)$/;
  for (let i = 0; i < lines.length; i++) {
    const m = re.exec(lines[i]);
    if (m) {
      out.push({ name: m[1], value: m[2].trim(), line: i + 1 });
    }
  }
  return out;
}

function scanImages(content: string): { path: string; line: number }[] {
  const out: { path: string; line: number }[] = [];
  const lines = content.split("\n");
  // Match `image:path[]` (inline) and `image::path[]` (block). The block form
  // is the common case for documentation; inline is rarer. Both share the
  // same `path` extraction.
  const re = /image:{1,2}([^\[\]]+)\[/g;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    let m: RegExpExecArray | null;
    re.lastIndex = 0;
    while ((m = re.exec(line)) !== null) {
      out.push({ path: m[1], line: i + 1 });
    }
  }
  return out;
}

/**
 * Mount the AsciiDoc parse-request listener. Call once at the app root.
 *
 * The listener receives `asciidoc:parse-requested` events from Rust, runs
 * `extractFacts`, and submits the result via `submitAsciiDocFacts`. The
 * entire IPC round-trip is wrapped in try/catch — if `submitAsciiDocFacts`
 * itself throws (serialization or IPC failure), a second submission with a
 * minimal error payload is attempted so the Rust coordinator is never left
 * with a dangling `inflight_adoc_count`.
 */
export function useAsciiDocParser(): void {
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    listen<AsciiDocParseRequested>("asciidoc:parse-requested", async (event) => {
      if (cancelled) return;
      const { documentId, version, content } = event.payload;

      let facts: AsciiDocFacts;
      try {
        facts = await extractFacts(content);
      } catch (e) {
        facts = {
          anchors: [],
          includes: [],
          references: [],
          attributes: [],
          images: [],
          parseErrors: [
            {
              message: e instanceof Error ? e.message : String(e),
              line: null,
              severity: "error",
            },
          ],
        };
      }

      try {
        await submitAsciiDocFacts(documentId, version, facts);
      } catch (submitError) {
        // The submit itself failed (IPC error, serialization issue). Attempt
        // a second submission with a minimal error payload so the coordinator
        // can decrement its inflight counter and drain the queue.
        const emptyFacts: AsciiDocFacts = {
          anchors: [],
          includes: [],
          references: [],
          attributes: [],
          images: [],
          parseErrors: [
            {
              message: `IPC submit failed: ${
                submitError instanceof Error ? submitError.message : String(submitError)
              }`,
              line: null,
              severity: "error",
            },
          ],
        };
        try {
          await submitAsciiDocFacts(documentId, version, emptyFacts);
        } catch {
          // Truly stuck — the Rust-side timeout (PARSE_TIMEOUT_SECS) is the
          // safety net. Nothing more we can do here.
        }
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
        return;
      }
      unlisten = fn;
    });

    // Signal Rust that the frontend is ready to receive parse requests.
    // Buffered requests from before this point will be drained.
    frontendReady().catch(() => {
      // Ignore — Rust side will keep requests buffered and retry on the
      // next dispatch. The timeout will eventually fire for any that never
      // get submitted.
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);
}
