import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, ChevronDown, ChevronRight, Copy, Plus, Trash2 } from "lucide-react";
import {
  artifactRender,
  type ArtifactContent,
  type JiraTicketSpec,
  type TicketLink,
} from "../../lib/artifacts";
import { toMessage } from "../../lib/errors";
import "./JiraTicketBuilder.css";

type TicketContent = Extract<ArtifactContent, { kind: "jiraTicket" }>;

type JiraTicketBuilderProps = {
  spec: TicketContent;
  onChange: (content: ArtifactContent) => void;
};

/** Mirrors `HttpRequestBuilder`: the preview is a pure Rust function with no
 *  I/O, so the debounce is about not doing an IPC round-trip per keystroke,
 *  not about cost. */
const PREVIEW_DEBOUNCE_MS = 200;

type OuterTabId = "builder" | "result";

const OUTER_TABS: { id: OuterTabId; label: string }[] = [
  { id: "builder", label: "Черновик" },
  { id: "result", label: "Разметка для Jira" },
];

type ProseKey = "why" | "outcome" | "solution";
type ListKey = "inScope" | "outOfScope" | "acceptanceCriteria" | "definitionOfDone" | "risks";

type SectionDef =
  | { kind: "prose"; key: ProseKey; label: string; hint: string; minRows: number }
  | { kind: "list"; key: ListKey; label: string; hint: string; minRows: number };

/** The eight text sections, in the order `domain::artifact_render` emits
 *  them. Labels are the tracker's own section names, so the draft reads as
 *  the ticket it will become. */
const SECTIONS: SectionDef[] = [
  {
    kind: "prose",
    key: "why",
    label: "Почему задача существует",
    hint: "Проблема и её причина, без решения",
    minRows: 2,
  },
  {
    kind: "prose",
    key: "outcome",
    label: "Что должно измениться после задачи",
    hint: "«Пользователь может …», а не «Реализовать …»",
    minRows: 1,
  },
  {
    kind: "list",
    key: "inScope",
    label: "Что входит в задачу",
    hint: "Законченные действия, а не «доработать логику»",
    minRows: 2,
  },
  {
    kind: "list",
    key: "outOfScope",
    label: "Что не входит в задачу",
    hint: "То, что легко спутать со скоупом",
    minRows: 1,
  },
  {
    kind: "prose",
    key: "solution",
    label: "Техническое решение",
    hint: "Endpoint или подход. Спецификацию — ссылкой",
    minRows: 1,
  },
  {
    kind: "list",
    key: "acceptanceCriteria",
    label: "Критерии приемки (AC)",
    hint: "Один сценарий на пункт: есть «и» — скорее всего, это два пункта",
    minRows: 3,
  },
  {
    kind: "list",
    key: "definitionOfDone",
    label: "Критерии выполнения (DoD)",
    hint: "Только специфичное для этой задачи: метрики, тесты, документация",
    minRows: 2,
  },
  {
    kind: "list",
    key: "risks",
    label: "Риски",
    hint: "Только содержательные. Нет риска — оставьте пустым",
    minRows: 1,
  },
];

/** The one structured section, which lives after the text ones and is
 *  numbered in the same running sequence. */
const LINKS_LABEL = "Ссылки";

export function JiraTicketBuilder({ spec, onChange }: JiraTicketBuilderProps) {
  const [outerTab, setOuterTab] = useState<OuterTabId>("builder");
  const [wiki, setWiki] = useState("");
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  // Links are conditional by the skill's own rules — only when the user
  // supplied any — so the section stays folded until it holds something.
  const [extrasOpen, setExtrasOpen] = useState(spec.links.length > 0);

  const patch = useCallback(
    (fields: Partial<JiraTicketSpec>) => onChange({ ...spec, ...fields }),
    [onChange, spec],
  );

  // Same debounce-then-render loop the HTTP builder uses, so the markup a
  // user copies is the identical string the assistant received — produced
  // by the one Rust renderer, never reimplemented here.
  const contentKey = useMemo(() => JSON.stringify(spec), [spec]);
  const generationRef = useRef(0);
  useEffect(() => {
    const generation = ++generationRef.current;
    const timer = setTimeout(() => {
      void (async () => {
        try {
          const rendered = await artifactRender(spec);
          if (generation !== generationRef.current) return;
          setWiki(rendered.kind === "jiraTicket" ? rendered.wiki : "");
          setPreviewError(null);
        } catch (e) {
          if (generation !== generationRef.current) return;
          setPreviewError(toMessage(e));
        }
      })();
    }, PREVIEW_DEBOUNCE_MS);
    return () => clearTimeout(timer);
    // `contentKey` stands in for `spec` — a fresh object identity on every
    // keystroke would restart the timer even when nothing actually changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [contentKey]);

  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (copiedTimerRef.current !== null) clearTimeout(copiedTimerRef.current);
    },
    [],
  );

  const handleCopy = async () => {
    await writeText(wiki);
    setCopied(true);
    if (copiedTimerRef.current !== null) clearTimeout(copiedTimerRef.current);
    copiedTimerRef.current = setTimeout(() => setCopied(false), 2000);
  };

  const numbers = sectionNumbers(spec);
  const extrasCount = countLinks(spec.links);

  return (
    <div className="ticket-builder">
      <div className="ticket-builder-outer-tabs" role="tablist">
        {OUTER_TABS.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={outerTab === tab.id}
            className={`ticket-builder-outer-tab${outerTab === tab.id ? " is-active" : ""}`}
            onClick={() => setOuterTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {outerTab === "builder" ? (
        // Laid out as the ticket rather than as a form: the numbered bold
        // headings, their order, and the fact that an empty section simply
        // disappears are all things the author needs to see while writing,
        // and a column of labelled boxes hides every one of them.
        <div className="ticket-doc">
          {SECTIONS.map((section) => {
            const value =
              section.kind === "list" ? spec[section.key].join("\n") : spec[section.key];
            const number = numbers[section.key];
            return (
              <section
                key={section.key}
                className={`ticket-doc-section${number ? "" : " is-empty"}`}
              >
                <SectionHeading label={section.label} number={number} />
                <AutoTextarea
                  className="ticket-doc-body"
                  aria-label={section.label}
                  placeholder={section.hint}
                  minRows={section.minRows}
                  value={value}
                  onChange={(next) =>
                    patch(
                      (section.kind === "list"
                        ? { [section.key]: next.split("\n") }
                        : { [section.key]: next }) as Partial<JiraTicketSpec>,
                    )
                  }
                />
                {section.kind === "list" ? (
                  <span className="ticket-doc-note">
                    Каждая строка — отдельный пункт списка
                  </span>
                ) : null}
              </section>
            );
          })}

          <div className="ticket-extras">
            <button
              type="button"
              className="ticket-extras-toggle"
              aria-expanded={extrasOpen}
              onClick={() => setExtrasOpen((open) => !open)}
            >
              {extrasOpen ? (
                <ChevronDown size={14} aria-hidden />
              ) : (
                <ChevronRight size={14} aria-hidden />
              )}
              <span>{LINKS_LABEL}</span>
              <span className="ticket-extras-hint">
                {extrasCount > 0 ? `заполнено: ${extrasCount}` : "необязательно"}
              </span>
            </button>

            {extrasOpen ? (
              <div className="ticket-extras-body">
                <LinkList
                  links={spec.links}
                  number={numbers.links}
                  onChange={(links) => patch({ links })}
                />
              </div>
            ) : null}
          </div>
        </div>
      ) : (
        <div className="ticket-builder-preview">
          <div className="ticket-preview-actions">
            <button type="button" className="artifact-btn" onClick={() => void handleCopy()}>
              {copied ? <Check size={13} aria-hidden /> : <Copy size={13} aria-hidden />}
              {copied ? "Скопировано" : "Копировать"}
            </button>
            <span className="ticket-preview-note">
              Разметка Jira (wiki markup), а не Markdown — вставляется в описание задачи как есть.
            </span>
          </div>
          {previewError ? <p className="artifact-view-error">{previewError}</p> : null}
          {wiki ? (
            <pre className="ticket-preview-code">{wiki}</pre>
          ) : (
            <p className="ticket-preview-empty">
              Пока нечего показать — заполните хотя бы один раздел.
            </p>
          )}
        </div>
      )}
    </div>
  );
}

/** A filled section carries its number, exactly as it will appear in Jira;
 *  an empty one is dimmed and says it will not be there at all. */
function SectionHeading({ label, number }: { label: string; number: number | null }) {
  return (
    <span className="ticket-doc-heading">
      {number ? <span className="ticket-doc-number">{number}.</span> : null}
      <span className="ticket-doc-title">{label}</span>
      {number ? null : <span className="ticket-doc-skip">не попадёт в задачу</span>}
    </span>
  );
}

/** Live numbering that mirrors `domain::artifact_render`: numbers are
 *  assigned after the empty sections are dropped, so filling one in
 *  renumbers everything below it — and the author watches that happen
 *  instead of discovering it after pasting into Jira. `null` means the
 *  section will not be rendered. */
export function sectionNumbers(spec: TicketContent): Record<string, number | null> {
  const filled: [string, boolean][] = [
    ["why", spec.why.trim().length > 0],
    ["outcome", spec.outcome.trim().length > 0],
    ["inScope", countItems(spec.inScope) > 0],
    ["outOfScope", countItems(spec.outOfScope) > 0],
    ["solution", spec.solution.trim().length > 0],
    ["acceptanceCriteria", countItems(spec.acceptanceCriteria) > 0],
    ["definitionOfDone", countItems(spec.definitionOfDone) > 0],
    ["risks", countItems(spec.risks) > 0],
    ["links", countLinks(spec.links) > 0],
  ];

  const numbers: Record<string, number | null> = {};
  let next = 1;
  for (const [key, isFilled] of filled) {
    numbers[key] = isFilled ? next++ : null;
  }
  return numbers;
}

/** Blank lines are counted out rather than stripped from the value: a blank
 *  line is what someone pressing Enter is in the middle of typing, and the
 *  renderer drops them anyway when the ticket is produced. */
function countItems(items: string[]): number {
  return items.filter((item) => item.trim().length > 0).length;
}

/** Matches the renderer: a link without a URL is not a link. */
function countLinks(links: TicketLink[]): number {
  return links.filter((l) => l.url.trim()).length;
}

/** A field that is exactly as tall as its text — no inner scrollbar, and
 *  nothing clipped.
 *
 *  Counting `\n` is not enough: one long line wraps to several visual rows,
 *  and with the overflow hidden that this layout needs, the overflow would
 *  be invisibly cut off rather than merely scrollable. So the height is
 *  measured (`scrollHeight`) instead of computed, after resetting to `auto`
 *  so shrinking works too. The `rows` attribute stays as the floor: with
 *  `height: auto` the box is already `minRows` tall, and `scrollHeight`
 *  never reports less than that. */
function AutoTextarea({
  value,
  onChange,
  minRows,
  ...rest
}: {
  value: string;
  onChange: (value: string) => void;
  minRows: number;
  className?: string;
  placeholder?: string;
  "aria-label"?: string;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);

  const resize = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    // `scrollHeight` covers content + padding but not the border, and the
    // box is `border-box`; without adding it back the field ends up a
    // couple of pixels short and scrolls after all.
    const borders = el.offsetHeight - el.clientHeight;
    if (el.scrollHeight > 0) el.style.height = `${el.scrollHeight + borders}px`;
  }, []);

  useLayoutEffect(resize, [resize, value]);

  // Re-wrapping on a width change (the dock resized, the window resized)
  // changes the height without changing the value, so the effect above
  // would not fire.
  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(resize);
    observer.observe(el);
    return () => observer.disconnect();
  }, [resize]);

  return (
    <textarea
      {...rest}
      ref={ref}
      rows={minRows}
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

function LinkList({
  links,
  number,
  onChange,
}: {
  links: TicketLink[];
  number: number | null;
  onChange: (links: TicketLink[]) => void;
}) {
  const replace = (index: number, patch: Partial<TicketLink>) =>
    onChange(links.map((link, i) => (i === index ? { ...link, ...patch } : link)));

  return (
    <section className={`ticket-doc-section${number ? "" : " is-empty"}`}>
      <SectionHeading label={LINKS_LABEL} number={number} />
      {links.length > 0 ? (
        <ul className="ticket-rows" role="list">
          {links.map((link, index) => (
            <li key={index} className="ticket-row">
              {/* Free text rather than a fixed set: GIT / CONFLUENCE / FIGMA
                  cover the tracker's tickets, but a fourth type must not
                  need a code change to be writable. No `datalist` — its
                  suggestion popup is drawn by the platform (see AGENTS.md,
                  Style → UI); the three usual values are named in the hint
                  under the field instead. */}
              <input
                className="ticket-input ticket-input-narrow"
                value={link.kind}
                placeholder="GIT"
                aria-label={`Тип ссылки ${index + 1}`}
                onChange={(e) => replace(index, { kind: e.target.value })}
              />
              <input
                className="ticket-input"
                value={link.url}
                placeholder="https://…"
                aria-label={`URL ссылки ${index + 1}`}
                onChange={(e) => replace(index, { url: e.target.value })}
              />
              {/* Hand-written tickets carry bare URLs, so this column is not
                  offered by default — but it appears once something is in it
                  (the assistant may set one), rather than editing around a
                  value the form does not admit exists. */}
              {link.title ? (
                <input
                  className="ticket-input"
                  value={link.title}
                  placeholder="Название"
                  aria-label={`Название ссылки ${index + 1}`}
                  onChange={(e) => replace(index, { title: e.target.value })}
                />
              ) : null}
              <button
                type="button"
                className="ticket-icon-btn"
                title="Удалить ссылку"
                aria-label={`Удалить ссылку ${index + 1}`}
                onClick={() => onChange(links.filter((_, i) => i !== index))}
              >
                <Trash2 size={13} aria-hidden />
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      <button
        type="button"
        className="ticket-add-btn"
        onClick={() => onChange([...links, { kind: "", url: "", title: "" }])}
      >
        <Plus size={13} aria-hidden />
        Добавить ссылку
      </button>
    </section>
  );
}
