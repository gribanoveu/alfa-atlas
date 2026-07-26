import { useMemo } from "react";
import type * as Monaco from "monaco-editor";
import ReactMarkdown from "react-markdown";
import type { Components } from "react-markdown";
import rehypeSlug from "rehype-slug";
import remarkGfm from "remark-gfm";
import { AscMermaid } from "../AsciiDocPreview/AscMermaid";
import type { AbstractBlock } from "../AsciiDocPreview/types";
import "../AsciiDocPreview/AsciiDocPreview.css";
import { MdCodeBlock } from "./MdCodeBlock";
import { MdImage } from "./MdImage";
import "./MarkdownPreview.css";

type XrefHandler = (href: string) => void;

type MarkdownPreviewProps = {
  content: string;
  filePath: string | null;
  docsRoot: string | null;
  monaco: typeof Monaco | null;
  onOpenXref?: XrefHandler;
};

function makeMermaidBlock(source: string): AbstractBlock {
  return {
    getSource: () => source,
    getAttribute: () => null,
  } as unknown as AbstractBlock;
}

function isExternalHref(href: string): boolean {
  return /^https?:\/\//i.test(href) || href.startsWith("mailto:");
}

function fencedLang(className: string | undefined): string | null {
  const match = /language-(\S+)/.exec(className ?? "");
  return match ? match[1].toLowerCase() : null;
}

/**
 * Markdown preview for `.md`/`.markdown` files.
 * GFM via remark-gfm; heading slugs via rehype-slug for anchor links.
 */
export function MarkdownPreview({
  content,
  docsRoot,
  monaco,
  onOpenXref,
}: MarkdownPreviewProps) {
  const components = useMemo((): Components => {
    return {
      a({ href, children, ...props }) {
        if (!href || isExternalHref(href) || !onOpenXref) {
          return (
            <a href={href} {...props}>
              {children}
            </a>
          );
        }
        return (
          <a
            href={href}
            {...props}
            onClick={(event) => {
              event.preventDefault();
              onOpenXref(href);
            }}
          >
            {children}
          </a>
        );
      },
      img({ src, alt }) {
        return <MdImage src={src} alt={alt} docsRoot={docsRoot} />;
      },
      code({ className, children, ...props }) {
        const inline = !className?.includes("language-");
        if (inline) {
          return (
            <code className="md-inline-code" {...props}>
              {children}
            </code>
          );
        }

        const rawLang = fencedLang(className);
        const source = String(children).replace(/\n$/, "");

        if (rawLang === "mermaid") {
          return (
            <AscMermaid
              block={makeMermaidBlock(source)}
              docsRoot={docsRoot}
            />
          );
        }

        return (
          <MdCodeBlock source={source} rawLang={rawLang} monaco={monaco} />
        );
      },
      pre({ children }) {
        return <>{children}</>;
      },
    };
  }, [docsRoot, monaco, onOpenXref]);

  if (!content.trim()) {
    return <div className="asc-preview asc-preview-empty">Нет содержимого</div>;
  }

  return (
    <div className="asc-preview md-preview">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeSlug]}
        components={components}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}
