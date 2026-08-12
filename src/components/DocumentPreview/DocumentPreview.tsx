import type * as Monaco from "monaco-editor";
import {
  isJsonPath,
  isMarkdownPath,
  isYamlPath,
} from "../../lib/fileExtensions";
import { isImageAsset } from "../../lib/supportedFiles";
import { AsciiDocPreview } from "../AsciiDocPreview/AsciiDocPreview";
import { MarkdownPreview } from "../MarkdownPreview/MarkdownPreview";
import { StructuredDataPreview } from "../StructuredDataPreview/StructuredDataPreview";
import { ImagePreview } from "./ImagePreview";

type XrefHandler = (href: string) => void;

type DocumentPreviewProps = {
  content: string;
  filePath: string | null;
  docsRoot: string | null;
  monaco: typeof Monaco | null;
  onOpenXref?: XrefHandler;
};

/** Routes preview by file extension: image, Markdown, JSON/YAML, vs AsciiDoc. */
export function DocumentPreview(props: DocumentPreviewProps) {
  if (props.filePath && isImageAsset(props.filePath)) {
    return (
      <ImagePreview relativePath={props.filePath} docsRoot={props.docsRoot} />
    );
  }
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
