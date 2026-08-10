import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, Copy } from "lucide-react";
import { isValidElement, useState } from "react";
import type { ReactNode } from "react";
import type { Components } from "streamdown";
import { Streamdown } from "streamdown";
import { wrapAsciiTrees } from "../../lib/wrapAsciiTrees";
import "./AssistantMarkdown.css";

function fencedLang(className: string | undefined): string | null {
  const match = /language-(\S+)/.exec(className ?? "");
  return match ? match[1].toLowerCase() : null;
}

/** Recovers a fenced code block's plain text from its rendered React
 * children (always a single string in practice, but walked defensively —
 * `code`'s children come from Streamdown's own markdown parse, not user
 * input we control) so `CodeCopyButton` can hand the clipboard raw text
 * instead of whatever DOM nodes ended up on screen. */
function childrenToText(node: ReactNode): string {
  if (typeof node === "string") return node;
  if (typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(childrenToText).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return childrenToText(node.props.children);
  return "";
}

/** Small icon button pinned to a fenced code block's corner (see
 * `.markdown-code-block` in AssistantMarkdown.css) — uses the same
 * `@tauri-apps/plugin-clipboard-manager` `writeText` the rest of the app
 * copies through (e.g. `JsonExample`), not the browser `navigator.clipboard`
 * Streamdown's own built-in copy button relies on, which isn't guaranteed
 * to work from inside the Tauri webview. */
function CodeCopyButton({ code }: { code: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard unavailable — silently ignore, the code is still visible
      // and selectable by hand.
    }
  };

  return (
    <button
      type="button"
      className={`markdown-code-copy-btn${copied ? " markdown-code-copy-btn-copied" : ""}`}
      title={copied ? "Скопировано" : "Копировать код"}
      onClick={() => void handleCopy()}
    >
      {copied ? <Check size={13} aria-hidden /> : <Copy size={13} aria-hidden />}
    </button>
  );
}

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
  code: ({ className, children }) => {
    if (!fencedLang(className)) {
      return <code className="markdown-code-inline">{children}</code>;
    }
    return (
      <pre className="markdown-code-block">
        <CodeCopyButton code={childrenToText(children)} />
        <code>{children}</code>
      </pre>
    );
  },
};

type AssistantMarkdownProps = {
  content: string;
  streaming: boolean;
};

/** Renders one assistant chat message's Markdown — headless Streamdown (no
 * Tailwind, see `components` above), chosen for `remend`'s streaming-safe
 * incomplete-Markdown handling while `streaming` is true. */
export function AssistantMarkdown({ content, streaming }: AssistantMarkdownProps) {
  return (
    <Streamdown className="assistant-md" isAnimating={streaming} linkSafety={{ enabled: false }} components={components}>
      {wrapAsciiTrees(content)}
    </Streamdown>
  );
}
