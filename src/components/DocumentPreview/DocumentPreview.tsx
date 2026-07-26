import type * as Monaco from "monaco-editor";
import { isMarkdownPath } from "../../lib/fileExtensions";
import { AsciiDocPreview } from "../AsciiDocPreview/AsciiDocPreview";
import { MarkdownPreview } from "../MarkdownPreview/MarkdownPreview";

type XrefHandler = (href: string) => void;

type DocumentPreviewProps = {
  content: string;
  filePath: string | null;
  docsRoot: string | null;
  monaco: typeof Monaco | null;
  onOpenXref?: XrefHandler;
};

/** Routes preview by file extension: Markdown vs AsciiDoc/diagrams. */
export function DocumentPreview(props: DocumentPreviewProps) {
  if (props.filePath && isMarkdownPath(props.filePath)) {
    return <MarkdownPreview {...props} />;
  }
  return <AsciiDocPreview {...props} />;
}
