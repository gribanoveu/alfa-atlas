import { useMemo, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy } from "lucide-react";
import {
  decodeBase64String,
  encodeBase64String,
  type Base64Alphabet,
} from "../../lib/base64Codec";
import { TEXT_ENCODING_OPTIONS, type TextEncodingId } from "../../lib/textEncoding";
import { UtilityInputHead } from "./UtilityClearButton";
import "./Base64Codec.css";

type CodecTab = "encode" | "decode";

const TABS: { id: CodecTab; label: string; desc: string }[] = [
  {
    id: "decode",
    label: "Base64 → текст",
    desc: "Декодирует Base64 в текст с автоопределением кодировки (UTF-8, Windows-1251 из XML и др.)",
  },
  {
    id: "encode",
    label: "Текст → Base64",
    desc: "Кодирует UTF-8 строку в Base64",
  },
];

const SAMPLE_ENCODE = "Привет, atlas!";
const SAMPLE_DECODE = "0J/RgNC40LLQtdGCLCBkb2NmbG93IQ==";

function formatBytes(size: number): string {
  if (size < 1024) {
    return `${size} B`;
  }
  return `${(size / 1024).toFixed(1)} KB`;
}

export function Base64Codec() {
  const [tab, setTab] = useState<CodecTab>("decode");
  const [raw, setRaw] = useState("");
  const [alphabet, setAlphabet] = useState<Base64Alphabet>("standard");
  const [padding, setPadding] = useState(true);
  const [encoding, setEncoding] = useState<TextEncodingId>("auto");
  const [copied, setCopied] = useState(false);

  const activeTab = TABS.find((item) => item.id === tab) ?? TABS[0];

  const result = useMemo(() => {
    if (tab === "encode") {
      return encodeBase64String(raw, { alphabet, padding });
    }
    return decodeBase64String(raw, { alphabet, encoding });
  }, [raw, tab, alphabet, padding, encoding]);

  const handleCopy = async () => {
    if (!result.ok) {
      return;
    }

    try {
      await writeText(result.output);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Буфер недоступен — результат остаётся на экране.
    }
  };

  const showError = raw.length > 0 && !result.ok;
  const inputLabel = tab === "encode" ? "Текст" : "Base64";
  const outputLabel = tab === "encode" ? "Base64" : "Текст";

  return (
    <div className="b64-codec">
      <div className="b64-tabs" role="tablist" aria-label="Режим Base64">
        {TABS.map((item) => (
          <button
            key={item.id}
            type="button"
            role="tab"
            aria-selected={tab === item.id}
            className={`b64-tab${tab === item.id ? " is-active" : ""}`}
            onClick={() => setTab(item.id)}
          >
            {item.label}
          </button>
        ))}
      </div>

      <div className="b64-panel">
        <p className="b64-panel-desc">{activeTab.desc}</p>

        <div className="b64-toolbar">
          <div className="b64-group">
            <span className="b64-group-label">Алфавит</span>
            <div className="b64-segmented" role="tablist" aria-label="Алфавит Base64">
              <button
                type="button"
                role="tab"
                aria-selected={alphabet === "standard"}
                className={`b64-segment${alphabet === "standard" ? " is-active" : ""}`}
                onClick={() => setAlphabet("standard")}
              >
                Standard
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={alphabet === "url"}
                className={`b64-segment${alphabet === "url" ? " is-active" : ""}`}
                onClick={() => setAlphabet("url")}
              >
                URL-safe
              </button>
            </div>
          </div>

          {tab === "encode" ? (
            <div className="b64-group">
              <span className="b64-group-label">Padding</span>
              <div className="b64-segmented" role="tablist" aria-label="Padding Base64">
                <button
                  type="button"
                  role="tab"
                  aria-selected={padding}
                  className={`b64-segment${padding ? " is-active" : ""}`}
                  onClick={() => setPadding(true)}
                >
                  Да
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={!padding}
                  className={`b64-segment${!padding ? " is-active" : ""}`}
                  onClick={() => setPadding(false)}
                >
                  Нет
                </button>
              </div>
            </div>
          ) : (
            <div className="b64-group">
              <span className="b64-group-label">Кодировка</span>
              <div className="b64-segmented" role="tablist" aria-label="Кодировка текста">
                {TEXT_ENCODING_OPTIONS.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    role="tab"
                    aria-selected={encoding === item.id}
                    className={`b64-segment${encoding === item.id ? " is-active" : ""}`}
                    onClick={() => setEncoding(item.id)}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>

        <div className="b64-input-wrap">
          <UtilityInputHead
            label={inputLabel}
            onClear={() => setRaw("")}
            clearDisabled={!raw}
          />
          <textarea
            className="b64-input"
            value={raw}
            onChange={(event) => setRaw(event.target.value)}
            placeholder={tab === "encode" ? SAMPLE_ENCODE : SAMPLE_DECODE}
            spellCheck={false}
            aria-label={inputLabel}
          />
        </div>

        {showError ? (
          <p className="b64-error" role="status">
            {result.reason}
          </p>
        ) : null}

        {result.ok && raw.length > 0 ? (
          <>
            <div className="b64-meta">
              <span>
                Вход: <strong>{formatBytes(result.bytesIn)}</strong>
              </span>
              <span>
                Выход: <strong>{formatBytes(result.bytesOut)}</strong>
              </span>
              {tab === "decode" && result.encodingLabel ? (
                <span>
                  Кодировка: <strong>{result.encodingLabel}</strong>
                </span>
              ) : null}
            </div>

            <div className="b64-output-block">
              <div className="b64-output-head">
                <h3 className="b64-section-title">{outputLabel}</h3>
                <button
                  type="button"
                  className={`b64-copy-btn${copied ? " is-copied" : ""}`}
                  onClick={() => void handleCopy()}
                  aria-label="Скопировать результат"
                  title={copied ? "Скопировано" : "Скопировать результат"}
                >
                  {copied ? (
                    <Check size={13} strokeWidth={2} aria-hidden />
                  ) : (
                    <Copy size={13} strokeWidth={1.75} aria-hidden />
                  )}
                </button>
              </div>
              <pre className="b64-output" aria-label="Результат">
                {result.output}
              </pre>
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}
