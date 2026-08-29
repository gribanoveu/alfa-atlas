import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy } from "lucide-react";
import { isValidElement, useEffect, useState } from "react";
import type { ReactNode } from "react";
import type { ThemedToken } from "shiki";
import { useIsCodeFenceIncomplete } from "streamdown";
import {
  highlightCodeWithShiki,
  splitCodeLines,
  themedTokenStyle,
} from "../../lib/shikiHighlight";

function fencedLang(className: string | undefined): string | null {
  const match = /language-(\S+)/.exec(className ?? "");
  return match ? match[1].toLowerCase() : null;
}

/** Recovers a fenced code block's plain text from its rendered React children. */
function childrenToText(node: ReactNode): string {
  if (typeof node === "string") return node;
  if (typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(childrenToText).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return childrenToText(node.props.children);
  return "";
}

function CodeCopyButton({ code }: { code: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard unavailable — code remains visible and selectable.
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

function PlainCodeLine({ line }: { line: string }) {
  return line || "\u00a0";
}

function HighlightedCodeLine({ tokens }: { tokens: ThemedToken[] | undefined }) {
  if (!tokens || tokens.length === 0) return "\u00a0";
  return (
    <>
      {tokens.map((token, index) => (
        <span key={index} style={themedTokenStyle(token)}>
          {token.content}
        </span>
      ))}
    </>
  );
}

type AssistantCodeBlockProps = {
  className?: string;
  children?: ReactNode;
};

/** Fenced code block for assistant chat — line numbers always, Shiki colors only
 * once the fence is complete so streaming stays synchronous and flicker-free. */
export function AssistantCodeBlock({ className, children }: AssistantCodeBlockProps) {
  const rawLang = fencedLang(className);
  const isFenced = rawLang !== null;
  const source = isFenced ? childrenToText(children) : "";
  const lines = splitCodeLines(source);
  const incomplete = useIsCodeFenceIncomplete();
  const [tokens, setTokens] = useState<ThemedToken[][] | null>(null);

  useEffect(() => {
    if (!isFenced || incomplete) {
      setTokens(null);
      return;
    }

    let cancelled = false;
    void highlightCodeWithShiki(source, rawLang).then((highlighted) => {
      if (!cancelled) setTokens(highlighted);
    });
    return () => {
      cancelled = true;
    };
  }, [source, rawLang, incomplete, isFenced]);

  if (!isFenced) {
    return <code className="markdown-code-inline">{children}</code>;
  }

  const showHighlight = !incomplete && tokens !== null;

  return (
    <div className="markdown-code-block" data-lang={rawLang}>
      <CodeCopyButton code={source} />
      <div className="markdown-code-scroll">
        <div className="markdown-code-body">
          {lines.map((line, index) => (
            <div key={index} className="markdown-code-line">
              <span className="markdown-code-gutter" aria-hidden>
                {index + 1}
              </span>
              <code className="markdown-code-content">
                {showHighlight ? (
                  <HighlightedCodeLine tokens={tokens[index]} />
                ) : (
                  <PlainCodeLine line={line} />
                )}
              </code>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
