import { useCallback, useMemo, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy } from "lucide-react";
import { decodeUlidTimestamp, generateUlidBatch } from "../../lib/ulid";
import { generateUuidV4Batch } from "../../lib/uuid";
import "./IdGenerator.css";

type GeneratorTab = "uuid" | "ulid";

const TABS: { id: GeneratorTab; label: string; desc: string }[] = [
  {
    id: "uuid",
    label: "UUID",
    desc: "Случайные идентификаторы UUID v4",
  },
  {
    id: "ulid",
    label: "ULID",
    desc: "Сортируемые идентификаторы с меткой времени",
  },
];

const COUNT_OPTIONS = [1, 5, 10] as const;

function ResultRow({
  id,
  label,
  value,
  copiedId,
  onCopy,
}: {
  id: string;
  label: string;
  value: string;
  copiedId: string | null;
  onCopy: (id: string, value: string) => void;
}) {
  const copied = copiedId === id;
  return (
    <div className="idgen-row">
      <span className="idgen-row-label">{label}</span>
      <code className="idgen-row-value">{value}</code>
      <button
        type="button"
        className={`idgen-row-copy${copied ? " is-copied" : ""}`}
        onClick={() => onCopy(id, value)}
        aria-label={`Скопировать: ${label}`}
        title={copied ? "Скопировано" : "Копировать"}
      >
        {copied ? (
          <Check size={13} strokeWidth={2} aria-hidden />
        ) : (
          <Copy size={13} strokeWidth={1.75} aria-hidden />
        )}
      </button>
    </div>
  );
}

export function IdGenerator() {
  const [tab, setTab] = useState<GeneratorTab>("uuid");
  const [count, setCount] = useState<(typeof COUNT_OPTIONS)[number]>(1);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [uuids, setUuids] = useState(() => generateUuidV4Batch(1));
  const [ulids, setUlids] = useState(() => generateUlidBatch(1));

  const activeTab = TABS.find((item) => item.id === tab) ?? TABS[0];
  const values = tab === "uuid" ? uuids : ulids;

  const ulidMeta = useMemo(() => {
    if (tab !== "ulid" || ulids.length === 0) return null;
    const timestamp = decodeUlidTimestamp(ulids[0]!);
    if (timestamp === null) return null;
    return new Date(timestamp).toLocaleString();
  }, [tab, ulids]);

  const regenerate = useCallback(() => {
    if (tab === "uuid") {
      setUuids(generateUuidV4Batch(count));
      return;
    }
    setUlids(generateUlidBatch(count));
  }, [count, tab]);

  const handleCountChange = (next: (typeof COUNT_OPTIONS)[number]) => {
    setCount(next);
    if (tab === "uuid") {
      setUuids(generateUuidV4Batch(next));
      return;
    }
    setUlids(generateUlidBatch(next));
  };

  const handleTabChange = (next: GeneratorTab) => {
    setTab(next);
  };

  const handleCopy = async (id: string, value: string) => {
    try {
      await writeText(value);
      setCopiedId(id);
      setTimeout(() => setCopiedId((current) => (current === id ? null : current)), 1500);
    } catch {
      // Буфер недоступен — значение всё равно видно на экране и выделяется мышью.
    }
  };

  const handleCopyAll = async () => {
    try {
      await writeText(values.join("\n"));
      setCopiedId("all");
      setTimeout(() => setCopiedId((current) => (current === "all" ? null : current)), 1500);
    } catch {
      // см. handleCopy
    }
  };

  return (
    <div className="idgen">
      <div className="idgen-tabs" role="tablist" aria-label="Тип идентификатора">
        {TABS.map((item) => (
          <button
            key={item.id}
            type="button"
            role="tab"
            id={`idgen-tab-${item.id}`}
            className={`idgen-tab${tab === item.id ? " is-active" : ""}`}
            aria-selected={tab === item.id}
            aria-controls={`idgen-panel-${item.id}`}
            onClick={() => handleTabChange(item.id)}
          >
            {item.label}
          </button>
        ))}
      </div>

      <section
        className="idgen-panel"
        role="tabpanel"
        id={`idgen-panel-${tab}`}
        aria-labelledby={`idgen-tab-${tab}`}
      >
        <p className="idgen-panel-desc">{activeTab.desc}</p>

        <div className="idgen-form-row">
          <span className="idgen-form-label">Количество</span>
          <div className="idgen-segmented" role="group" aria-label="Сколько идентификаторов сгенерировать">
            {COUNT_OPTIONS.map((option) => (
              <button
                key={option}
                type="button"
                className={`idgen-segment${count === option ? " is-active" : ""}`}
                aria-pressed={count === option}
                onClick={() => handleCountChange(option)}
              >
                {option}
              </button>
            ))}
          </div>
        </div>

        <div className="idgen-form-row">
          <span className="idgen-form-label">Действие</span>
          <div className="idgen-toolbar">
            <button type="button" className="idgen-btn" onClick={regenerate}>
              Обновить
            </button>
            {values.length > 1 ? (
              <button
                type="button"
                className={`idgen-btn${copiedId === "all" ? " is-copied" : ""}`}
                onClick={() => void handleCopyAll()}
              >
                {copiedId === "all" ? "Скопировано" : "Копировать все"}
              </button>
            ) : null}
          </div>
        </div>

        {tab === "uuid" ? (
          <p className="idgen-meta">
            Формат: <strong>UUID v4</strong>, нижний регистр
          </p>
        ) : (
          <p className="idgen-meta">
            {ulidMeta ? (
              <>
                Время из ULID: <strong>{ulidMeta}</strong>
              </>
            ) : (
              <>
                Формат: <strong>ULID</strong>, верхний регистр
              </>
            )}
          </p>
        )}

        <div className="idgen-rows">
          {values.map((value, index) => (
            <ResultRow
              key={`${tab}-${index}-${value}`}
              id={`${tab}-${index}`}
              label={values.length === 1 ? "Значение" : `#${index + 1}`}
              value={value}
              copiedId={copiedId}
              onCopy={handleCopy}
            />
          ))}
        </div>
      </section>
    </div>
  );
}
