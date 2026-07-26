import type * as Monaco from "monaco-editor";
import {
  isJsonPath,
  isMarkdownPath,
  isYamlPath,
} from "../../lib/fileExtensions";
import { AsciiDocPreview } from "../AsciiDocPreview/AsciiDocPreview";
import { MarkdownPreview } from "../MarkdownPreview/MarkdownPreview";
import { StructuredDataPreview } from "../StructuredDataPreview/StructuredDataPreview";

type XrefHandler = (href: string) => void;

type DocumentPreviewProps = {
  content: string;
  filePath: string | null;
  docsRoot: string | null;
  monaco: typeof Monaco | null;
  onOpenXref?: XrefHandler;
};

/** Routes preview by file extension: Markdown, JSON/YAML, vs AsciiDoc/diagrams. */
export function DocumentPreview(props: DocumentPreviewProps) {
  if (props.filePath && isMarkdownPath(props.filePath)) {
    return <MarkdownPreview {...props} />;
  }
  if (
    props.filePath &&
    (isJsonPath(props.filePath) || isYamlPath(props.filePath))
  ) {
    return <StructuredDataPreview {...props} />;
  }
  return <AsciiDocPreview {...props} />;
}
