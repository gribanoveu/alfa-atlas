import { useMemo, useState } from "react";
import { CopyTextButton } from "../Common/CopyTextButton";
import {
  decodeUnix,
  formatIsoLocal,
  formatIsoUtc,
  formatLocale,
  formatLocaleUtc,
  formatSeconds,
  localTimeZoneName,
  partsFromDate,
  partsFromStrings,
  partsToUnix,
  type DateZone,
  type UnixUnitMode,
} from "../../lib/unixtime";
import { UtilityClearButton, UtilityFieldShell } from "./UtilityClearButton";
import "./UnixtimeConverter.css";

type ConverterTab = "decode" | "encode";

type FieldsState = {
  year: string;
  month: string;
  day: string;
  hour: string;
  minute: string;
  second: string;
  millisecond: string;
};

type FieldDef = { key: keyof FieldsState; label: string };

const TABS: { id: ConverterTab; label: string; desc: string }[] = [
  {
    id: "decode",
    label: "Unixtime → дата",
    desc: "Введите число — получите дату в разных форматах",
  },
  {
    id: "encode",
    label: "Дата → Unixtime",
    desc: "Соберите дату и время — получите timestamp",
  },
];

const UNIT_MODES: { id: UnixUnitMode; label: string }[] = [
  { id: "auto", label: "Авто" },
  { id: "seconds", label: "Секунды" },
  { id: "milliseconds", label: "Миллисекунды" },
];

const ZONES: { id: DateZone; label: string }[] = [
  { id: "local", label: "Локальная" },
  { id: "utc", label: "UTC" },
];

const DATE_FIELDS: FieldDef[] = [
  { key: "year", label: "Год" },
  { key: "month", label: "Месяц" },
  { key: "day", label: "День" },
];

const TIME_FIELDS: FieldDef[] = [
  { key: "hour", label: "Часы" },
  { key: "minute", label: "Мин" },
  { key: "second", label: "Сек" },
  { key: "millisecond", label: "Мс" },
];

function fieldsFromDate(date: Date, zone: DateZone): FieldsState {
  const parts = partsFromDate(date, zone);
  return {
    year: String(parts.year),
    month: String(parts.month),
    day: String(parts.day),
    hour: String(parts.hour),
    minute: String(parts.minute),
    second: String(parts.second),
    millisecond: String(parts.millisecond),
  };
}

const EMPTY_FIELDS: FieldsState = {
  year: "",
  month: "",
  day: "",
  hour: "",
  minute: "",
  second: "",
  millisecond: "",
};

function fieldsAreEmpty(fields: FieldsState): boolean {
  return Object.values(fields).every((value) => value.trim().length === 0);
}

// Rows used to carry an `id` purely to disambiguate which one was ticked —
// two sections share labels, so a shared `copiedId` lit both. `CopyTextButton`
// keeps that state per button, so the id is gone with it.
function ResultRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="unix-row">
      <span className="unix-row-label">{label}</span>
      <code className="unix-row-value">{value}</code>
      <CopyTextButton
        text={value}
        className="unix-row-copy"
        label="Копировать"
        ariaLabel={`Скопировать: ${label}`}
        size={13}
      />
    </div>
  );
}

function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  ariaLabel,
}: {
  options: { id: T; label: string }[];
  value: T;
  onChange: (id: T) => void;
  ariaLabel: string;
}) {
  return (
    <div className="unix-segmented" role="group" aria-label={ariaLabel}>
      {options.map((option) => (
        <button
          key={option.id}
          type="button"
          className={`unix-segment${option.id === value ? " is-active" : ""}`}
          aria-pressed={option.id === value}
          onClick={() => onChange(option.id)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function FieldInput({
  field,
  value,
  onChange,
}: {
  field: FieldDef;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className={`unix-field unix-field-${field.key}`}>
      <span className="unix-field-label">{field.label}</span>
      <input
        className="unix-field-input"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        inputMode="numeric"
        spellCheck={false}
      />
    </label>
  );
}

export function UnixtimeConverter() {
  const [now] = useState(() => new Date());
  const timeZone = useMemo(() => localTimeZoneName(), []);
  const [tab, setTab] = useState<ConverterTab>("decode");

  const [raw, setRaw] = useState(() => String(Math.floor(now.getTime() / 1000)));
  const [unitMode, setUnitMode] = useState<UnixUnitMode>("auto");

  const [zone, setZone] = useState<DateZone>("local");
  const [fields, setFields] = useState<FieldsState>(() => fieldsFromDate(now, "local"));

  const decoded = useMemo(() => decodeUnix(raw, unitMode), [raw, unitMode]);

  const encoded = useMemo(() => {
    const parts = partsFromStrings(fields);
    if (!parts) {
      return { ok: false as const, reason: "Заполните все поля целыми числами" };
    }
    return partsToUnix(parts, zone);
  }, [fields, zone]);

  const activeTab = TABS.find((item) => item.id === tab) ?? TABS[0];

  // Смена зоны не должна менять момент времени: пересобираем поля из текущей
  // даты, иначе «14:00 локально» молча стало бы «14:00 UTC».
  const handleZoneChange = (next: DateZone) => {
    if (next === zone) return;
    if (encoded.ok) setFields(fieldsFromDate(encoded.value.date, next));
    setZone(next);
  };

  const setField = (key: keyof FieldsState, value: string) => {
    setFields((prev) => ({ ...prev, [key]: value }));
  };

  return (
    <div className="unix-converter">
      <div className="unix-tabs" role="tablist" aria-label="Режим конвертера">
        {TABS.map((item) => (
          <button
            key={item.id}
            type="button"
            role="tab"
            id={`unix-tab-${item.id}`}
            className={`unix-tab${tab === item.id ? " is-active" : ""}`}
            aria-selected={tab === item.id}
            aria-controls={`unix-panel-${item.id}`}
            onClick={() => setTab(item.id)}
          >
            {item.label}
          </button>
        ))}
      </div>

      <section
        className="unix-panel"
        role="tabpanel"
        id={`unix-panel-${tab}`}
        aria-labelledby={`unix-tab-${tab}`}
      >
        <p className="unix-panel-desc">{activeTab.desc}</p>

        {tab === "decode" ? (
          <>
            <div className="unix-form">
              <div className="unix-form-row">
                <span className="unix-form-label">Единица</span>
                <SegmentedControl
                  options={UNIT_MODES}
                  value={unitMode}
                  onChange={setUnitMode}
                  ariaLabel="Единица измерения ввода"
                />
              </div>

              <div className="unix-form-row">
                <label className="unix-form-label" htmlFor="unix-timestamp-input">
                  Timestamp
                </label>
                <div className="unix-input-row">
                  <UtilityFieldShell
                    variant="inline"
                    onClear={() => setRaw("")}
                    clearDisabled={!raw}
                    clearLabel="Очистить timestamp"
                  >
                    <input
                      id="unix-timestamp-input"
                      className="unix-input utility-field-control"
                      value={raw}
                      onChange={(event) => setRaw(event.target.value)}
                      placeholder="1700000000 или 1700000000000"
                      spellCheck={false}
                      aria-label="Unix-время"
                    />
                  </UtilityFieldShell>
                  <button
                    type="button"
                    className="unix-btn"
                    onClick={() => setRaw(String(Date.now()))}
                  >
                    Сейчас
                  </button>
                </div>
              </div>
            </div>

            {decoded.ok ? (
              <div className="unix-output">
                <div className="unix-highlight">
                  <span className="unix-highlight-label">Дата и время</span>
                  <p className="unix-highlight-value">{formatLocale(decoded.value.date)}</p>
                  <p className="unix-highlight-meta">
                    {decoded.value.autoDetected ? "Определено как " : "Читается как "}
                    <strong>
                      {decoded.value.unit === "seconds"
                        ? "Unix timestamp (секунды)"
                        : "Timestamp (миллисекунды)"}
                    </strong>
                  </p>
                </div>

                <div className="unix-rows">
                  <ResultRow
                    label="ISO 8601 (UTC)"
                    value={formatIsoUtc(decoded.value.date)}
                  />
                  <ResultRow
                    label={`ISO 8601 (${timeZone})`}
                    value={formatIsoLocal(decoded.value.date)}
                  />
                  <ResultRow
                    label="Locale (локальная)"
                    value={formatLocale(decoded.value.date)}
                  />
                  <ResultRow
                    label="Locale (UTC)"
                    value={formatLocaleUtc(decoded.value.date)}
                  />
                  <ResultRow
                    label="Unix (сек)"
                    value={formatSeconds(decoded.value.seconds)}
                  />
                  <ResultRow
                    label="Timestamp (мс)"
                    value={String(decoded.value.milliseconds)}
                  />
                </div>
              </div>
            ) : (
              <p className="unix-error" role="status">
                {decoded.reason}
              </p>
            )}
          </>
        ) : (
          <>
            <div className="unix-form">
              <div className="unix-form-row unix-form-row-toolbar">
                <div className="unix-form-row-main">
                  <span className="unix-form-label">Часовой пояс</span>
                  <SegmentedControl
                    options={ZONES}
                    value={zone}
                    onChange={handleZoneChange}
                    ariaLabel="Часовой пояс вводимой даты"
                  />
                </div>
                <div className="unix-form-row-actions">
                  <button
                    type="button"
                    className="unix-btn"
                    onClick={() => setFields(fieldsFromDate(new Date(), zone))}
                  >
                    Сейчас
                  </button>
                  <UtilityClearButton
                    onClear={() => setFields(EMPTY_FIELDS)}
                    disabled={fieldsAreEmpty(fields)}
                    label="Очистить дату и время"
                  />
                </div>
              </div>

              <div className="unix-datetime">
                <fieldset className="unix-datetime-group">
                  <legend className="unix-datetime-legend">Дата</legend>
                  <div className="unix-datetime-fields unix-datetime-fields-date">
                    {DATE_FIELDS.map((field) => (
                      <FieldInput
                        key={field.key}
                        field={field}
                        value={fields[field.key]}
                        onChange={(value) => setField(field.key, value)}
                      />
                    ))}
                  </div>
                </fieldset>

                <fieldset className="unix-datetime-group">
                  <legend className="unix-datetime-legend">Время</legend>
                  <div className="unix-datetime-fields unix-datetime-fields-time">
                    {TIME_FIELDS.map((field) => (
                      <FieldInput
                        key={field.key}
                        field={field}
                        value={fields[field.key]}
                        onChange={(value) => setField(field.key, value)}
                      />
                    ))}
                  </div>
                </fieldset>
              </div>

              <p className="unix-note">
                Интерпретация:{" "}
                <strong>{zone === "utc" ? "UTC" : timeZone}</strong>
              </p>
            </div>

            {encoded.ok ? (
              <div className="unix-output">
                <div className="unix-rows">
                  <ResultRow
                    label="Unix (сек)"
                    value={formatSeconds(encoded.value.seconds)}
                  />
                  <ResultRow
                    label="Timestamp (мс)"
                    value={String(encoded.value.milliseconds)}
                  />
                  <ResultRow
                    label="ISO 8601 (UTC)"
                    value={formatIsoUtc(encoded.value.date)}
                  />
                </div>
              </div>
            ) : (
              <p className="unix-error" role="status">
                {encoded.reason}
              </p>
            )}
          </>
        )}
      </section>
    </div>
  );
}
