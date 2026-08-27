import { useMemo, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy } from "lucide-react";
import type { HttpStatusCode } from "../../data/httpStatusCodes";
import {
  countHttpStatusMatches,
  filterHttpStatusGroups,
  HTTP_STATUS_FILTERS,
  type HttpStatusFilter,
} from "../../lib/httpStatusCodes";
import { UtilityFieldShell } from "./UtilityClearButton";
import "./HttpStatusReference.css";

function StatusItem({
  entry,
  copiedId,
  onCopy,
}: {
  entry: HttpStatusCode;
  copiedId: string | null;
  onCopy: (id: string, value: string) => void;
}) {
  const copyId = String(entry.code);
  const copied = copiedId === copyId;

  return (
    <article className="http-status-item">
      <span className={`http-status-code cat-${entry.category}`}>{entry.code}</span>
      <div className="http-status-body">
        <h3 className="http-status-name">{entry.name}</h3>
        <p className="http-status-desc">{entry.description}</p>
        <p className="http-status-usage">{entry.usage}</p>
      </div>
      <button
        type="button"
        className={`http-status-copy${copied ? " is-copied" : ""}`}
        onClick={() => onCopy(copyId, String(entry.code))}
        aria-label={`Скопировать код ${entry.code}`}
        title={copied ? "Скопировано" : "Копировать код"}
      >
        {copied ? (
          <Check size={13} strokeWidth={2} aria-hidden />
        ) : (
          <Copy size={13} strokeWidth={1.75} aria-hidden />
        )}
      </button>
    </article>
  );
}

export function HttpStatusReference() {
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<HttpStatusFilter>("all");
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const groups = useMemo(
    () => filterHttpStatusGroups(query, category),
    [query, category],
  );
  const matchCount = useMemo(
    () => countHttpStatusMatches(query, category),
    [query, category],
  );

  const handleCopy = async (id: string, value: string) => {
    try {
      await writeText(value);
      setCopiedId(id);
      setTimeout(() => setCopiedId((current) => (current === id ? null : current)), 1500);
    } catch {
      // Буфер недоступен — код всё равно виден на экране.
    }
  };

  return (
    <div className="http-status">
      <div className="http-status-toolbar">
        <UtilityFieldShell
          variant="inline"
          onClear={() => setQuery("")}
          clearDisabled={!query}
          clearLabel="Очистить поиск"
        >
          <input
            className="http-status-search utility-field-control"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Поиск по коду, названию, описанию или совету"
            spellCheck={false}
            aria-label="Поиск HTTP-кода"
          />
        </UtilityFieldShell>

        <div className="http-status-filters" role="group" aria-label="Класс HTTP-кода">
          {HTTP_STATUS_FILTERS.map((filter) => (
            <button
              key={filter.id}
              type="button"
              className={`http-status-filter${category === filter.id ? " is-active" : ""}`}
              aria-pressed={category === filter.id}
              onClick={() => setCategory(filter.id)}
            >
              {filter.label}
            </button>
          ))}
        </div>
      </div>

      <p className="http-status-meta">
        {matchCount === 1 ? "Найден 1 код" : `Найдено кодов: ${matchCount}`}
      </p>

      {groups.length > 0 ? (
        <div className="http-status-groups">
          {groups.map((group) => (
            <section key={group.id} aria-label={group.title}>
              <h2 className="http-status-group-title">{group.title}</h2>
              <div className="http-status-list">
                {group.codes.map((entry) => (
                  <StatusItem
                    key={entry.code}
                    entry={entry}
                    copiedId={copiedId}
                    onCopy={handleCopy}
                  />
                ))}
              </div>
            </section>
          ))}
        </div>
      ) : (
        <p className="http-status-empty" role="status">
          Ничего не найдено
        </p>
      )}
    </div>
  );
}
