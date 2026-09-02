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
import { collectTableShapes } from "../lib/asciidocTableModel";
import { toMessage } from "../lib/errors";

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
    tables: [],
  };

  // Capture parse-time warnings/errors via a MemoryLogger so genuine syntax
  // issues surface as ParseError diagnostics.
  const logger = new MemoryLogger();
  const previousLogger = LoggerManager.getLogger();
  LoggerManager.setLogger(logger);

  try {
    const doc = await load(content, {
      sourcemap: true,
      safe: "safe",
      attributes: { showtitle: true },
      // Per-parse registry — see `createIncludeCapturingRegistry`.
      extension_registry: createIncludeCapturingRegistry(capturedIncludes),
    });

    facts.includes = capturedIncludes.slice();
    // Reads the document already parsed above — a tree walk, no extra `load`
    // and no file access, so it costs the same order as the line scans below.
    // Deliberately the un-normalized parse: `useAsciiDocRender` puts content
    // through `normalizeBarePipeTables` first and so hides exactly the
    // mangling this is meant to surface.
    facts.tables = collectTableShapes(doc);
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
      message: toMessage(e),
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
  // Lines inside verbatim delimited blocks (listing `----`, literal `....`,
  // passthrough `++++`, comment `////`) are treated literally by asciidoctor —
  // `<<PK>>` inside a plantuml/source block is NOT an xref. Skip them.
  const verbatimLines = collectVerbatimLineIndices(lines);
  // Match `xref:target#anchor[...]` or `xref:target[...]`. Target may be a
  // path (with optional `#fragment`) or just `#fragment` for same-doc
  // anchors.
  //
  // Only the *opening* bracket is required, the way `scanImages` already
  // does it. Requiring `\[\]` meant every xref carrying link text —
  // `xref:a.adoc[Настройка]`, which is the ordinary form in real
  // documentation — was invisible here, so neither its broken target nor
  // its broken anchor was ever reported. The angle-bracket branch below
  // always accepted text; the two forms had simply drifted apart.
  const re = /xref:([^\[\]]+?)(?:#([^\[\]]+))?\[/g;
  // Angle-bracket short form: `<<target#anchor,text>>`, `<<target,text>>`,
  // `<<#anchor,text>>`, `<<target#anchor>>`, `<<target>>`, `<<#anchor>>`.
  // Target/anchor stop at `#`, `,` or `>`. Target is `*` (not `+`) so that a
  // leading `#anchor` form matches with an empty target.
  const shortRe = /<<([^,>#]*)(?:#([^,>#]+))?(?:,[^>]*)?>>/g;
  for (let i = 0; i < lines.length; i++) {
    if (verbatimLines.has(i)) continue;
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

/**
 * Delimiter line for a verbatim AsciiDoc block: 4+ of `-` (listing), `.`
 * (literal), `+` (passthrough), or `/` (comment block), optionally followed
 * by trailing whitespace. Example/sidebar/quote/open-block delimiters are
 * intentionally NOT matched here — their content is AsciiDoc markup, so
 * `<<...>>` inside them is a legitimate xref.
 */
const VERBATIM_DELIM_RE = /^(-{4,}|\.{4,}|\+{4,}|\/{4,})\s*$/;

/**
 * Return the set of 0-based line indices that fall strictly inside a verbatim
 * delimited block (listing / literal / passthrough / comment). Delimiter lines
 * themselves are not included. Unclosed blocks are treated as verbatim to EOF
 * (asciidoctor is lenient, and we prefer false negatives over false positives
 * here).
 */
function collectVerbatimLineIndices(lines: string[]): Set<number> {
  const indices = new Set<number>();
  let delimChar: string | null = null;
  let blockStart = -1;
  for (let i = 0; i < lines.length; i++) {
    if (delimChar === null) {
      const m = VERBATIM_DELIM_RE.exec(lines[i]);
      if (m) {
        delimChar = m[1][0];
        blockStart = i;
      }
    } else {
      const closingRe = new RegExp(`^\\${delimChar}{4,}\\s*$`);
      if (closingRe.test(lines[i])) {
        for (let j = blockStart + 1; j < i; j++) indices.add(j);
        delimChar = null;
        blockStart = -1;
      }
    }
  }
  if (delimChar !== null && blockStart >= 0) {
    for (let j = blockStart + 1; j < lines.length; j++) indices.add(j);
  }
  return indices;
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
              message: toMessage(e),
              line: null,
              severity: "error",
            },
          ],
          tables: [],
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
                toMessage(submitError)
              }`,
              line: null,
              severity: "error",
            },
          ],
          tables: [],
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
