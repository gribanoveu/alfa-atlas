import { useState } from "react";
import { Check, Download } from "lucide-react";
import { AscCodeBlock } from "../AsciiDocPreview/AscCodeBlock";
import { AscMermaid } from "../AsciiDocPreview/AscMermaid";
import { AscPlantuml } from "../AsciiDocPreview/AscPlantuml";
import { makeDiagramBlock } from "../AsciiDocPreview/syntheticBlock";
import { saveBytesViaDialog } from "../../lib/fileSave";
import { toMessage } from "../../lib/errors";
import {
  DIAGRAM_FORMAT_EXTENSIONS,
  DIAGRAM_FORMAT_LABELS,
  type Visual,
} from "../../lib/visuals";
import "./VisualView.css";

type Pane = "render" | "source";

/** Filename seed for the save dialog: the title, ASCII-safe and hyphenated,
 *  falling back to a generic name when the title is all punctuation or a
 *  script the filesystem would rather not carry. */
function fileNameFor(visual: Visual): string {
  const slug = visual.title
    .toLowerCase()
    .replace(/[^a-z0-9а-яё]+/gi, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 60);
  const ext = DIAGRAM_FORMAT_EXTENSIONS[visual.content.format];
  return `${slug || "diagram"}.${ext}`;
}

/** The rendered diagram, through the very same viewers the AsciiDoc preview
 *  uses for `[mermaid]`/`[plantuml]` blocks and for standalone `.mmd`/
 *  `.puml` files — so zoom, pan, Fit and 1:1 behave identically wherever a
 *  diagram shows up in the app, and a render error reads the same too. */
function DiagramPane({ visual }: { visual: Visual }) {
  const block = makeDiagramBlock(visual.content.source, visual.title);
  return visual.content.format === "plantuml" ? (
    <AscPlantuml block={block} />
  ) : (
    <AscMermaid block={block} />
  );
}

/** One visualization's tab: a header (title, caption, render/source toggle,
 *  save) plus the per-kind renderer, dispatched the way `ArtifactView`
 *  dispatches on `content.kind`.
 *
 *  Read-only by design. A visualization is something the assistant said,
 *  not a document the user owns — so there is nothing to save back to and
 *  no dirty state; «Сохранить в файл» is the way out into a real file,
 *  which then opens through the ordinary preview path. */
export function VisualView({ visual }: { visual: Visual }) {
  const [pane, setPane] = useState<Pane>("render");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [justSaved, setJustSaved] = useState(false);

  const format = visual.content.format;
  const formatLabel = DIAGRAM_FORMAT_LABELS[format];

  const handleSave = async () => {
    setSaveError(null);
    try {
      const bytes = new TextEncoder().encode(visual.content.source);
      const saved = await saveBytesViaDialog(bytes, {
        defaultPath: fileNameFor(visual),
        filters: [{ name: formatLabel, extensions: [DIAGRAM_FORMAT_EXTENSIONS[format]] }],
      });
      // `false` is the user cancelling the dialog, not a failure.
      if (saved) {
        setJustSaved(true);
        setTimeout(() => setJustSaved(false), 2000);
      }
    } catch (e) {
      setSaveError(toMessage(e));
    }
  };

  return (
    <div className="visual-view">
      <header className="visual-view-head">
        <div className="visual-view-heading">
          <span className="visual-view-eyebrow">Визуализация · Схема · {formatLabel}</span>
          <div className="visual-view-title">{visual.title}</div>
        </div>
        <div className="visual-view-actions">
          <div className="visual-view-toggle" role="group" aria-label="Что показывать">
            <button
              type="button"
              className={`visual-toggle-btn${pane === "render" ? " is-active" : ""}`}
              aria-pressed={pane === "render"}
              onClick={() => setPane("render")}
            >
              Схема
            </button>
            <button
              type="button"
              className={`visual-toggle-btn${pane === "source" ? " is-active" : ""}`}
              aria-pressed={pane === "source"}
              onClick={() => setPane("source")}
            >
              Исходник
            </button>
          </div>
          <button type="button" className="visual-btn" onClick={() => void handleSave()}>
            {justSaved ? <Check size={13} aria-hidden /> : <Download size={13} aria-hidden />}
            {justSaved ? "Сохранено" : "Сохранить в файл"}
          </button>
        </div>
      </header>

      {visual.caption ? <p className="visual-view-caption">{visual.caption}</p> : null}

      {saveError ? <p className="visual-view-error">{saveError}</p> : null}

      <div className="visual-view-body">
        {pane === "source" ? (
          <div className="visual-view-source">
            {/* `monaco={null}`: Monaco has no `mermaid`/`plantuml` language
                registered, so it would fall through to plain text anyway —
                this skips threading an editor instance down here for a
                highlighter that could never fire. */}
            <AscCodeBlock block={makeDiagramBlock(visual.content.source, null)} monaco={null} />
          </div>
        ) : visual.content.kind === "diagram" ? (
          <DiagramPane visual={visual} />
        ) : null}
      </div>
    </div>
  );
}
