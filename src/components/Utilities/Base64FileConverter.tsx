import { useEffect, useMemo, useRef, useState } from "react";
import { Check, Copy } from "lucide-react";
import {
  createObjectUrl,
  decodeBase64FileInput,
  encodeFileBytesToBase64,
  readFileAsBytes,
  type BinaryContentKind,
} from "../../lib/base64File";
import {
  decodeBase64ToBytes,
  encodeBytesToBase64,
  type Base64Alphabet,
} from "../../lib/base64Codec";
import { copyToClipboard } from "../../lib/clipboard";
import { toMessage } from "../../lib/errors";
import { saveDecodedBinaryFile } from "../../lib/fileSave";
import { UtilityClearButton, UtilityLabeledField } from "./UtilityClearButton";
import "./Base64FileConverter.css";

type FileTab = "decode" | "encode";

const TABS: { id: FileTab; label: string; desc: string }[] = [
  {
    id: "decode",
    label: "Base64 → файл",
    desc: "Декодирует Base64 или data URI в файл. PDF и изображения можно посмотреть перед сохранением.",
  },
  {
    id: "encode",
    label: "Файл → Base64",
    desc: "Кодирует выбранный файл в Base64. Поддерживается data URI для результата.",
  },
];

const TINY_PNG_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAD0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

function formatBytes(size: number): string {
  if (size < 1024) {
    return `${size} B`;
  }
  return `${(size / 1024).toFixed(1)} KB`;
}

function isPreviewable(kind: BinaryContentKind): boolean {
  return kind === "pdf" || kind === "image";
}

export function Base64FileConverter() {
  const [tab, setTab] = useState<FileTab>("decode");
  const [raw, setRaw] = useState("");
  const [alphabet, setAlphabet] = useState<Base64Alphabet>("standard");
  const [padding, setPadding] = useState(true);
  const [dataUri, setDataUri] = useState(false);
  const [copied, setCopied] = useState(false);
  const [selectedFile, setSelectedFile] = useState<{ name: string; bytes: Uint8Array } | null>(
    null,
  );
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [justSaved, setJustSaved] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const activeTab = TABS.find((item) => item.id === tab) ?? TABS[0];

  const decoded = useMemo(() => {
    if (tab !== "decode") {
      return null;
    }
    return decodeBase64FileInput(raw, { alphabet }, decodeBase64ToBytes);
  }, [raw, tab, alphabet]);

  const encoded = useMemo(() => {
    if (tab !== "encode" || !selectedFile) {
      return null;
    }
    return encodeFileBytesToBase64(
      selectedFile.bytes,
      selectedFile.name,
      alphabet,
      padding,
      encodeBytesToBase64,
    );
  }, [tab, selectedFile, alphabet, padding]);

  const encodeOutput = useMemo(() => {
    if (!encoded?.ok) {
      return "";
    }
    if (!dataUri) {
      return encoded.base64;
    }
    return `data:${encoded.content.mime};base64,${encoded.base64}`;
  }, [encoded, dataUri]);

  useEffect(() => {
    if (!decoded?.ok || !isPreviewable(decoded.content.kind)) {
      setPreviewUrl(null);
      return;
    }

    const url = createObjectUrl(decoded.bytes, decoded.content.mime);
    setPreviewUrl(url);
    return () => {
      URL.revokeObjectURL(url);
    };
  }, [decoded]);

  const handleCopy = async (value: string) => {
    try {
      await copyToClipboard(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Буфер недоступен — результат остаётся на экране.
    }
  };

  const handlePickFile = () => {
    fileInputRef.current?.click();
  };

  const handleFileChange = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) {
      return;
    }

    setSelectedFile(await readFileAsBytes(file));
  };

  const handleSaveDecoded = async () => {
    if (!decoded?.ok) {
      return;
    }

    setSaveError(null);
    try {
      const saved = await saveDecodedBinaryFile(decoded.bytes, decoded.content);
      // `false` — пользователь закрыл диалог, это не ошибка.
      if (saved) {
        setJustSaved(true);
        setTimeout(() => setJustSaved(false), 2000);
      }
    } catch (e) {
      setSaveError(toMessage(e));
    }
  };

  const decodeError = tab === "decode" && raw.trim() && decoded && !decoded.ok ? decoded.reason : null;
  const encodeError =
    tab === "encode" && selectedFile && encoded && !encoded.ok ? encoded.reason : null;

  return (
    <div className="b64file">
      <div className="b64file-tabs" role="tablist" aria-label="Режим Base64 файлов">
        {TABS.map((item) => (
          <button
            key={item.id}
            type="button"
            role="tab"
            aria-selected={tab === item.id}
            className={`b64file-tab${tab === item.id ? " is-active" : ""}`}
            onClick={() => setTab(item.id)}
          >
            {item.label}
          </button>
        ))}
      </div>

      <div className="b64file-panel">
        <p className="b64file-panel-desc">{activeTab.desc}</p>

        <div className="b64file-toolbar">
          <div className="b64file-group">
            <span className="b64file-group-label">Алфавит</span>
            <div className="b64file-segmented" role="tablist" aria-label="Алфавит Base64">
              <button
                type="button"
                role="tab"
                aria-selected={alphabet === "standard"}
                className={`b64file-segment${alphabet === "standard" ? " is-active" : ""}`}
                onClick={() => setAlphabet("standard")}
              >
                Standard
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={alphabet === "url"}
                className={`b64file-segment${alphabet === "url" ? " is-active" : ""}`}
                onClick={() => setAlphabet("url")}
              >
                URL-safe
              </button>
            </div>
          </div>

          {tab === "encode" ? (
            <>
              <div className="b64file-group">
                <span className="b64file-group-label">Padding</span>
                <div className="b64file-segmented" role="tablist" aria-label="Padding Base64">
                  <button
                    type="button"
                    role="tab"
                    aria-selected={padding}
                    className={`b64file-segment${padding ? " is-active" : ""}`}
                    onClick={() => setPadding(true)}
                  >
                    Да
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={!padding}
                    className={`b64file-segment${!padding ? " is-active" : ""}`}
                    onClick={() => setPadding(false)}
                  >
                    Нет
                  </button>
                </div>
              </div>

              <div className="b64file-group">
                <span className="b64file-group-label">Data URI</span>
                <div className="b64file-segmented" role="tablist" aria-label="Формат data URI">
                  <button
                    type="button"
                    role="tab"
                    aria-selected={!dataUri}
                    className={`b64file-segment${!dataUri ? " is-active" : ""}`}
                    onClick={() => setDataUri(false)}
                  >
                    Нет
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={dataUri}
                    className={`b64file-segment${dataUri ? " is-active" : ""}`}
                    onClick={() => setDataUri(true)}
                  >
                    Да
                  </button>
                </div>
              </div>
            </>
          ) : null}
        </div>

        {tab === "decode" ? (
          <UtilityLabeledField
            label="Base64 или data URI"
            onClear={() => setRaw("")}
            clearDisabled={!raw}
            clearLabel="Очистить Base64"
          >
            <textarea
              className="b64file-input utility-field-control"
              value={raw}
              onChange={(event) => {
                setRaw(event.target.value);
                setSaveError(null);
              }}
              placeholder={TINY_PNG_BASE64}
              spellCheck={false}
              aria-label="Base64 или data URI"
            />
          </UtilityLabeledField>
        ) : (
          <>
            <input
              ref={fileInputRef}
              className="b64file-file-input"
              type="file"
              onChange={(event) => void handleFileChange(event)}
              aria-hidden
              tabIndex={-1}
            />
            <div className="b64file-actions">
              <button type="button" className="b64file-action" onClick={handlePickFile}>
                Выбрать файл
              </button>
            </div>
            {selectedFile ? (
              <div className="b64file-selected-row">
                <p className="b64file-selected">
                  {selectedFile.name} · {formatBytes(selectedFile.bytes.length)}
                </p>
                <UtilityClearButton
                  onClear={() => setSelectedFile(null)}
                  label="Очистить выбранный файл"
                />
              </div>
            ) : null}
          </>
        )}

        {decodeError ? (
          <p className="b64file-error" role="status">
            {decodeError}
          </p>
        ) : null}

        {encodeError ? (
          <p className="b64file-error" role="status">
            {encodeError}
          </p>
        ) : null}

        {decoded?.ok && raw.trim() ? (
          <>
            <div className="b64file-meta">
              <span>
                Тип: <strong>{decoded.content.label}</strong>
              </span>
              <span>
                Размер: <strong>{formatBytes(decoded.bytes.length)}</strong>
              </span>
            </div>

            {previewUrl && decoded.content.kind === "image" ? (
              <div className="b64file-preview-block">
                <h3 className="b64file-section-title">Предпросмотр</h3>
                <img
                  src={previewUrl}
                  alt="Предпросмотр изображения"
                  className="b64file-preview-image"
                />
              </div>
            ) : null}

            {previewUrl && decoded.content.kind === "pdf" ? (
              <div className="b64file-preview-block">
                <h3 className="b64file-section-title">Предпросмотр</h3>
                <iframe
                  src={previewUrl}
                  title="Предпросмотр PDF"
                  className="b64file-preview-pdf"
                />
              </div>
            ) : null}

            <div className="b64file-actions">
              <button type="button" className="b64file-action" onClick={() => void handleSaveDecoded()}>
                {justSaved ? "Сохранено" : "Сохранить файл"}
              </button>
            </div>

            {saveError ? (
              <p className="b64file-error" role="status">
                Не удалось сохранить файл: {saveError}
              </p>
            ) : null}
          </>
        ) : null}

        {encoded?.ok ? (
          <>
            <div className="b64file-meta">
              <span>
                Тип: <strong>{encoded.content.label}</strong>
              </span>
              <span>
                Вход: <strong>{formatBytes(encoded.bytesIn)}</strong>
              </span>
              <span>
                Выход: <strong>{formatBytes(encoded.bytesOut)}</strong>
              </span>
            </div>

            <div className="b64file-output-block">
              <div className="b64file-output-head">
                <h3 className="b64file-section-title">Base64</h3>
                <button
                  type="button"
                  className={`b64file-copy-btn${copied ? " is-copied" : ""}`}
                  onClick={() => void handleCopy(encodeOutput)}
                  aria-label="Скопировать Base64"
                  title={copied ? "Скопировано" : "Скопировать Base64"}
                >
                  {copied ? (
                    <Check size={13} strokeWidth={2} aria-hidden />
                  ) : (
                    <Copy size={13} strokeWidth={1.75} aria-hidden />
                  )}
                </button>
              </div>
              <pre className="b64file-output" aria-label="Результат Base64">
                {encodeOutput}
              </pre>
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}
