import xmlFormat from "xml-formatter";

export type XmlIndent = 2 | 4;

export type XmlFormatMode = "prettify" | "minify";

export type XmlFormatOptions = {
  mode: XmlFormatMode;
  indent: XmlIndent;
};

export type XmlFormatResult =
  | {
      ok: true;
      output: string;
      bytesIn: number;
      bytesOut: number;
    }
  | { ok: false; reason: string };

const FORMAT_OPTIONS = {
  collapseContent: true,
  strictMode: true,
  throwOnFailure: true,
} as const;

export function formatXmlValue(text: string, options: XmlFormatOptions): string {
  if (options.mode === "minify") {
    return xmlFormat.minify(text, FORMAT_OPTIONS);
  }

  const output = xmlFormat(text, {
    ...FORMAT_OPTIONS,
    indentation: " ".repeat(options.indent),
    lineSeparator: "\n",
  });

  return output.endsWith("\n") ? output : `${output}\n`;
}

export function formatXmlInput(text: string, options: XmlFormatOptions): XmlFormatResult {
  const trimmed = text.trim();
  if (!trimmed) {
    return { ok: false, reason: "Введите XML" };
  }

  try {
    const output = formatXmlValue(trimmed, options);
    const bytesIn = new TextEncoder().encode(trimmed).length;
    const bytesOut = new TextEncoder().encode(output).length;

    return {
      ok: true,
      output,
      bytesIn,
      bytesOut,
    };
  } catch (error) {
    const message = error instanceof Error ? error.message : "Некорректный XML";
    return { ok: false, reason: message };
  }
}
