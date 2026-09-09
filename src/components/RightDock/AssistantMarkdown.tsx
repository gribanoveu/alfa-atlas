import { memo } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Components } from "streamdown";
import { Streamdown } from "streamdown";
import { wrapAsciiTrees } from "../../lib/wrapAsciiTrees";
import { AssistantCodeBlock } from "./AssistantCodeBlock";
import "./AssistantMarkdown.css";

// No Tailwind in this project (see AssistantPanel.css's own doc comment) —
// Streamdown's default per-element rendering leans on Tailwind utility
// classes that resolve to nothing here, so every element it can render gets
// its own override below, each carrying a plain `markdown-*` class styled in
// AssistantMarkdown.css (adapted from MarkdownPreview.css's `.md-preview`
// rules, at the chat bubble's smaller type scale).
const components: Components = {
  h1: ({ children }) => <h1 className="markdown-h1">{children}</h1>,
  h2: ({ children }) => <h2 className="markdown-h2">{children}</h2>,
  h3: ({ children }) => <h3 className="markdown-h3">{children}</h3>,
  h4: ({ children }) => <h4 className="markdown-h4">{children}</h4>,
  h5: ({ children }) => <h5 className="markdown-h5">{children}</h5>,
  h6: ({ children }) => <h6 className="markdown-h6">{children}</h6>,
  p: ({ children }) => <p className="markdown-p">{children}</p>,
  strong: ({ children }) => <strong className="markdown-strong">{children}</strong>,
  em: ({ children }) => <em className="markdown-em">{children}</em>,
  ul: ({ children }) => <ul className="markdown-ul">{children}</ul>,
  ol: ({ children }) => <ol className="markdown-ol">{children}</ol>,
  li: ({ children }) => <li className="markdown-li">{children}</li>,
  blockquote: ({ children }) => <blockquote className="markdown-blockquote">{children}</blockquote>,
  table: ({ children }) => <table className="markdown-table">{children}</table>,
  thead: ({ children }) => <thead className="markdown-thead">{children}</thead>,
  tbody: ({ children }) => <tbody className="markdown-tbody">{children}</tbody>,
  tr: ({ children }) => <tr className="markdown-tr">{children}</tr>,
  th: ({ children }) => <th className="markdown-th">{children}</th>,
  td: ({ children }) => <td className="markdown-td">{children}</td>,
  hr: () => <hr className="markdown-hr" />,
  sup: ({ children }) => <sup className="markdown-sup">{children}</sup>,
  sub: ({ children }) => <sub className="markdown-sub">{children}</sub>,
  img: ({ src, alt }) => <img className="markdown-img" src={src} alt={alt} />,
  a: ({ href, children }) => (
    <a
      href={href}
      className="markdown-link"
      onClick={(event) => {
        event.preventDefault();
        if (href) void openUrl(href);
      }}
    >
      {children}
    </a>
  ),
  pre: ({ children }) => <>{children}</>,
  code: ({ className, children }) => (
    <AssistantCodeBlock className={className}>{children}</AssistantCodeBlock>
  ),
};

type AssistantMarkdownProps = {
  content: string;
  streaming: boolean;
};

/** Renders one assistant chat message's Markdown — headless Streamdown (no
 * Tailwind, see `components` above), chosen for `remend`'s streaming-safe
 * incomplete-Markdown handling while `streaming` is true.
 *
 * That handling is tied to `streaming` rather than left on by default,
 * because it is a bet that the half-written markup will be finished by the
 * next delta — and it deletes text to make the bet. `Текст с ![картинкой и
 * хвост` renders as `Текст с`: everything after an unmatched `![` is
 * dropped, and an unmatched `[` loses its bracket and gains a stray blocked
 * link. Mid-stream that costs a frame; on a finished answer, where nothing
 * more is coming, it is permanent content loss — and brackets in prose
 * (`arr[0`, AsciiDoc macros) are ordinary here. */
/** `memo`, because this is the expensive half of a streaming turn. Every
 * delta event re-renders the whole transcript, and without this each past
 * message re-parses its Markdown (and re-runs Shiki inside
 * `AssistantCodeBlock`) for a token that changed only the last one. Both
 * props are primitives, so the default shallow comparison is enough. */
export const AssistantMarkdown = memo(function AssistantMarkdown({
  content,
  streaming,
}: AssistantMarkdownProps) {
  return (
    <Streamdown
      className="assistant-md"
      isAnimating={streaming}
      parseIncompleteMarkdown={streaming}
      linkSafety={{ enabled: false }}
      components={components}
    >
      {wrapAsciiTrees(content)}
    </Streamdown>
  );
});
