import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Building2, Check, ClipboardPaste, Copy, Plus, Trash2, Wand2 } from "lucide-react";
import {
  artifactRender,
  type ArtifactContent,
  type BodySpec,
  type ErrorSpec,
  type ParamSpec,
  type RenderedHttpRequest,
  type ResponseSpec,
} from "../../lib/artifacts";
import { toMessage } from "../../lib/errors";
import {
  BODY_HEADERS,
  HTTP_METHODS,
  corporateHeadersFor,
  emptyParam,
  ensureHeaders,
  inferParamsFromJson,
  mergeInferredParams,
  missingPathParams,
} from "../../lib/httpRequestSpec";
import { applyCurlImport, parseCurl } from "../../lib/curlImport";

/** Narrowed to its own variant — `ArtifactView` dispatches on
 *  `content.kind`, so this component only ever sees an httpRequest. */
type HttpRequestSpecContent = Extract<ArtifactContent, { kind: "httpRequest" }>;

type HttpRequestBuilderProps = {
  spec: HttpRequestSpecContent;
  onChange: (content: ArtifactContent) => void;
};

type SectionId = "path" | "query" | "headers" | "body" | "responses" | "errors";

const SECTIONS: { id: SectionId; label: string }[] = [
  { id: "path", label: "Path" },
  { id: "query", label: "Query" },
  { id: "headers", label: "Заголовки" },
  { id: "body", label: "Тело" },
  { id: "responses", label: "Ответы" },
  { id: "errors", label: "Ошибки" },
];

/** How long to wait after the last keystroke before re-rendering the
 *  preview. The render itself is a pure Rust function with no I/O, so this
 *  is about not doing an IPC round-trip per character, not about cost. */
const PREVIEW_DEBOUNCE_MS = 200;

const PREVIEW_TABS = [
  { id: "inputParams", label: "Входные параметры" },
  { id: "outputParams", label: "Выходные параметры" },
  { id: "curl", label: "curl" },
  { id: "requestAdoc", label: "request.adoc" },
  { id: "responseAdoc", label: "response.adoc" },
] as const;

type PreviewTabId = (typeof PREVIEW_TABS)[number]["id"];

type OuterTabId = "builder" | "result";

const OUTER_TABS: { id: OuterTabId; label: string }[] = [
  { id: "builder", label: "Конструктор" },
  { id: "result", label: "Результат" },
];

export function HttpRequestBuilder({ spec, onChange }: HttpRequestBuilderProps) {
  // Always starts on the constructor — `ArtifactView` remounts this
  // component per artifact tab (see `App.tsx`'s `key={activeArtifact}`,
  // and the editor pane itself unmounts a hidden tab's content rather than
  // just hiding it), so a fresh default here is enough; nothing needs to
  // reset it explicitly on reopen.
  const [outerTab, setOuterTab] = useState<OuterTabId>("builder");
  const [section, setSection] = useState<SectionId>("path");
  const [previewTab, setPreviewTab] = useState<PreviewTabId>("inputParams");
  const [rendered, setRendered] = useState<RenderedHttpRequest | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  // The preview is produced by the same renderer the assistant's tool
  // result goes through, so what the user approves here is exactly what
  // gets sent — a second TypeScript implementation would drift.
  useEffect(() => {
    let cancelled = false;
    const timer = setTimeout(() => {
      void (async () => {
        try {
          const next = await artifactRender(spec);
          if (!cancelled) {
            // The renderer is keyed by content kind, so this branch always
            // holds — narrowing here rather than casting keeps that true by
            // construction if a third kind ever arrives.
            setRendered(next.kind === "httpRequest" ? next : null);
            setPreviewError(null);
          }
        } catch (e) {
          if (!cancelled) setPreviewError(toMessage(e));
        }
      })();
    }, PREVIEW_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [spec]);

  const patch = useCallback(
    (changes: Partial<HttpRequestSpecContent>) => onChange({ ...spec, ...changes }),
    [onChange, spec],
  );

  // Импорт из curl: чаще всего работающий запрос у автора уже есть — из
  // DevTools, Postman или чужой инструкции, — и перенос его в форму руками
  // это просто переписывание того же самого.
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState("");
  const [importError, setImportError] = useState<string | null>(null);

  const applyImport = () => {
    const parsed = parseCurl(importText);
    if (!parsed) {
      setImportError(
        "Не похоже на curl: нужна команда, начинающаяся с curl и содержащая адрес запроса.",
      );
      return;
    }
    onChange({ kind: "httpRequest", ...applyCurlImport(spec, parsed) });
    setImportOpen(false);
    setImportText("");
    setImportError(null);
  };

  const undeclaredPathParams = useMemo(() => missingPathParams(spec), [spec]);

  const addMissingPathParams = () => {
    patch({
      pathParams: [
        ...spec.pathParams,
        ...undeclaredPathParams.map((name) => ({ ...emptyParam(), name })),
      ],
    });
    setSection("path");
  };

  const body = spec.body;
  const previewText = rendered ? rendered[previewTab] : "";

  const handleCopy = async () => {
    if (!previewText) return;
    try {
      await writeText(previewText);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Буфер недоступен — результат остаётся на экране.
    }
  };

  return (
    <div className="http-builder">
      {/* Same full-width top-tab shell as `Base64Codec`'s encode/decode
          switch (`.b64-tabs`/`.b64-tab`) — the app's established pattern
          for "this pane is one of two entirely different views", as
          opposed to the smaller `.http-builder-tabs` below, which switch
          sections within one view. */}
      <div className="http-builder-outer-tabs" role="tablist" aria-label="Раздел конструктора">
        {OUTER_TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={outerTab === t.id}
            className={`http-builder-outer-tab${outerTab === t.id ? " is-active" : ""}`}
            onClick={() => setOuterTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>

      {outerTab === "builder" ? (
        <div className="http-builder-form">
          <div className="http-builder-import-row">
            <button
              type="button"
              className="artifact-btn"
              aria-expanded={importOpen}
              onClick={() => {
                setImportError(null);
                setImportOpen((open) => !open);
              }}
            >
              <ClipboardPaste size={13} aria-hidden />
              Импорт из curl
            </button>
          </div>

          {importOpen ? (
            <div className="http-builder-import">
              <textarea
                className="http-builder-import-input"
                value={importText}
                spellCheck={false}
                rows={6}
                autoFocus
                placeholder={"curl -X POST 'https://host/api/documents' \\\n  -H 'Content-Type: application/json' \\\n  -d '{\"id\": 1}'"}
                aria-label="Команда curl"
                onChange={(e) => {
                  setImportText(e.target.value);
                  setImportError(null);
                }}
              />
              {importError ? (
                <p className="http-builder-import-error">{importError}</p>
              ) : null}
              <p className="http-builder-import-note">
                Заполнятся метод, адрес, query-параметры, заголовки и тело. Ответы, коды
                ошибок и описания полей в curl не содержатся — их дозаполняете вы.
                Уже введённые описания и форматы сохранятся. Значения авторизации,
                ключей и cookie заменяются плейсхолдером, чтобы токен со стенда не
                попал в документ.
              </p>
              <div className="http-builder-import-actions">
                <button
                  type="button"
                  className="artifact-btn"
                  onClick={() => {
                    setImportOpen(false);
                    setImportError(null);
                  }}
                >
                  Отмена
                </button>
                <button
                  type="button"
                  className="artifact-btn primary"
                  disabled={!importText.trim()}
                  onClick={applyImport}
                >
                  Заполнить
                </button>
              </div>
            </div>
          ) : null}

          <div className="http-builder-line">
            <MethodSelect value={spec.method || "GET"} onChange={(method) => patch({ method })} />
            <input
              className="http-builder-input http-builder-host"
              value={spec.baseUrl}
              placeholder="https://corp-gateway-test"
              aria-label="Хост"
              onChange={(e) => patch({ baseUrl: e.target.value })}
            />
            <input
              className="http-builder-input http-builder-path"
              value={spec.path}
              placeholder="/api/{organizationId}/documents"
              aria-label="Путь"
              onChange={(e) => patch({ path: e.target.value })}
            />
          </div>

          {undeclaredPathParams.length > 0 ? (
            <div className="http-builder-hint">
              <span>
                В пути есть неописанные параметры: {undeclaredPathParams.map((n) => `{${n}}`).join(", ")}
              </span>
              <button type="button" className="artifact-btn" onClick={addMissingPathParams}>
                Добавить их
              </button>
            </div>
          ) : null}

          <nav className="http-builder-tabs" role="tablist" aria-label="Разделы запроса">
            {SECTIONS.map((s) => (
              <button
                key={s.id}
                type="button"
                role="tab"
                aria-selected={section === s.id}
                className={`http-builder-tab${section === s.id ? " is-active" : ""}`}
                onClick={() => setSection(s.id)}
              >
                {s.label}
                <SectionCount id={s.id} spec={spec} />
              </button>
            ))}
          </nav>

          <div className="http-builder-section">
            {section === "path" ? (
              <ParamTable
                params={spec.pathParams}
                onChange={(pathParams) => patch({ pathParams })}
                emptyHint="Параметры пути — то, что подставляется в {фигурные скобки}."
              />
            ) : null}
            {section === "query" ? (
              <ParamTable
                params={spec.queryParams}
                onChange={(queryParams) => patch({ queryParams })}
                emptyHint="Параметры строки запроса, после «?»."
              />
            ) : null}
            {section === "headers" ? (
              <ParamTable
                params={spec.headers}
                onChange={(headers) => patch({ headers })}
                emptyHint="Заголовки запроса — например стандартный блок A-userId / A-customerId."
                onInsertCorporate={() =>
                  patch({
                    headers: ensureHeaders(spec.headers, corporateHeadersFor(body !== null)),
                  })
                }
              />
            ) : null}
            {section === "body" ? (
              <BodyEditor
                body={body}
                onChange={(next) =>
                  patch({
                    body: next,
                    // Тело без Content-Type/Accept — незаполненный контракт,
                    // поэтому пара добавляется вместе с ним, а не ждёт, пока
                    // о ней вспомнят. Только при появлении тела: если строку
                    // потом удалили осознанно, возвращать её не надо.
                    headers:
                      next !== null && body === null
                        ? ensureHeaders(spec.headers, BODY_HEADERS)
                        : spec.headers,
                  })
                }
              />
            ) : null}
            {section === "responses" ? (
              <ResponsesEditor
                responses={spec.responses}
                onChange={(responses) => patch({ responses })}
              />
            ) : null}
            {section === "errors" ? (
              <ErrorsEditor errors={spec.errors} onChange={(errors) => patch({ errors })} />
            ) : null}
          </div>
        </div>
      ) : (
        <aside className="http-builder-preview">
          <nav className="http-builder-tabs" role="tablist" aria-label="Предпросмотр">
            {PREVIEW_TABS.map((t) => (
              <button
                key={t.id}
                type="button"
                role="tab"
                aria-selected={previewTab === t.id}
                className={`http-builder-tab${previewTab === t.id ? " is-active" : ""}`}
                onClick={() => {
                  setPreviewTab(t.id);
                  setCopied(false);
                }}
              >
                {t.label}
              </button>
            ))}
          </nav>
          <div className="http-builder-preview-note">
            <p>Ровно то, что получит ассистент — можно скопировать прямо в документ.</p>
            <button
              type="button"
              className={`http-builder-copy-btn${copied ? " is-copied" : ""}`}
              onClick={() => void handleCopy()}
              disabled={!previewText}
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
          {previewError ? (
            <p className="artifact-view-error">{previewError}</p>
          ) : (
            <pre className="http-builder-preview-body">{previewText || "…"}</pre>
          )}
        </aside>
      )}
    </div>
  );
}

/** Trigger + absolute option list, closes on outside click / Escape — same
 * programmatic-dropdown pattern as `ServerSelect` (OpenApiExplorer) and
 * `ChatModeSelect` (AssistantConversation), not a native `<select>`, so it
 * renders consistently with the rest of the app instead of the browser's
 * own (unstyleable) menu chrome. */
function MethodSelect({ value, onChange }: { value: string; onChange: (method: string) => void }) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div className="method-select" ref={rootRef}>
      <button
        type="button"
        className={`method-select-trigger${open ? " is-open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label="HTTP-метод"
        onClick={() => setOpen((o) => !o)}
      >
        <span className="method-select-value">{value}</span>
        <span className="method-select-chevron" aria-hidden>
          ▾
        </span>
      </button>
      {open ? (
        <div className="method-select-menu" role="listbox">
          {HTTP_METHODS.map((method) => (
            <button
              key={method}
              type="button"
              role="option"
              aria-selected={method === value}
              className={`method-select-option${method === value ? " is-active" : ""}`}
              onClick={() => {
                onChange(method);
                setOpen(false);
              }}
            >
              {method}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function SectionCount({ id, spec }: { id: SectionId; spec: HttpRequestSpecContent }) {
  const count =
    id === "path"
      ? spec.pathParams.length
      : id === "query"
        ? spec.queryParams.length
        : id === "headers"
          ? spec.headers.length
          : id === "body"
            ? (spec.body?.params.length ?? 0)
            : id === "responses"
              ? spec.responses.length
              : spec.errors.length;
  if (count === 0) return null;
  return <span className="http-builder-tab-count">{count}</span>;
}

type ParamTableProps = {
  params: ParamSpec[];
  onChange: (params: ParamSpec[]) => void;
  emptyHint: string;
  /** Shown above the table when the rows can be seeded from a JSON sample. */
  onInferFromJson?: () => void;
  /** Заголовки: подставить корпоративный блок A-* (плюс пару для тела). */
  onInsertCorporate?: () => void;
};

/** The five columns of the house parameter table, in the same order the
 *  templates use — the form is deliberately a table, not a stack of
 *  labelled fields, because the output is a table and the user is checking
 *  rows against each other. */
/** A field that looks like a single-line input at rest but wraps and grows
 *  with its content instead of scrolling horizontally — parameter
 *  descriptions and example values routinely run long (a whole sentence, a
 *  pasted example), and silently clipping them is worse than a taller row.
 *  `rows={1}` plus resize-to-`scrollHeight` on every value change is the
 *  standard textarea-autosize technique; not worth a dependency for one
 *  field shape. `useLayoutEffect` (not `useEffect`) so the resize happens
 *  before paint — otherwise a value set from outside typing (loading a
 *  record, "Разобрать поля из JSON") would flash at the wrong height for a
 *  frame. */
function AutoGrowField({
  value,
  onChange,
  ariaLabel,
}: {
  value: string;
  onChange: (value: string) => void;
  ariaLabel: string;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);

  return (
    <textarea
      ref={ref}
      rows={1}
      className="param-table-field"
      value={value}
      aria-label={ariaLabel}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

function ParamTable({
  params,
  onChange,
  emptyHint,
  onInferFromJson,
  onInsertCorporate,
}: ParamTableProps) {
  const update = (index: number, changes: Partial<ParamSpec>) =>
    onChange(params.map((p, i) => (i === index ? { ...p, ...changes } : p)));

  return (
    <div className="param-table-wrap">
      <div className="param-table-actions">
        {onInferFromJson ? (
          <button type="button" className="artifact-btn" onClick={onInferFromJson}>
            <Wand2 size={13} aria-hidden />
            Разобрать поля из JSON
          </button>
        ) : null}
        {onInsertCorporate ? (
          <button type="button" className="artifact-btn" onClick={onInsertCorporate}>
            <Building2 size={13} aria-hidden />
            Корпоративные заголовки
          </button>
        ) : null}
        <button type="button" className="artifact-btn" onClick={() => onChange([...params, emptyParam()])}>
          <Plus size={13} aria-hidden />
          Добавить параметр
        </button>
      </div>

      {params.length === 0 ? (
        <p className="param-table-empty">{emptyHint}</p>
      ) : (
        <table className="param-table">
          <thead>
            <tr>
              <th>Параметр</th>
              <th>Формат</th>
              <th className="param-table-required">Обяз.</th>
              <th>Описание</th>
              <th>Варианты значений</th>
              <th aria-label="Удалить" />
            </tr>
          </thead>
          <tbody>
            {params.map((param, index) => (
              <tr key={index}>
                <td>
                  <AutoGrowField
                    value={param.name}
                    ariaLabel={`Имя параметра ${index + 1}`}
                    onChange={(name) => update(index, { name })}
                  />
                </td>
                <td>
                  <AutoGrowField
                    value={param.format}
                    ariaLabel={`Формат параметра ${index + 1}`}
                    onChange={(format) => update(index, { format })}
                  />
                </td>
                <td className="param-table-required">
                  <input
                    type="checkbox"
                    checked={param.required}
                    aria-label={`Обязательный параметр ${index + 1}`}
                    onChange={(e) => update(index, { required: e.target.checked })}
                  />
                </td>
                <td>
                  <AutoGrowField
                    value={param.description}
                    ariaLabel={`Описание параметра ${index + 1}`}
                    onChange={(description) => update(index, { description })}
                  />
                </td>
                <td>
                  <AutoGrowField
                    value={param.values}
                    ariaLabel={`Варианты значений параметра ${index + 1}`}
                    onChange={(values) => update(index, { values })}
                  />
                </td>
                <td>
                  <button
                    type="button"
                    className="param-table-remove"
                    aria-label={`Удалить параметр ${index + 1}`}
                    onClick={() => onChange(params.filter((_, i) => i !== index))}
                  >
                    <Trash2 size={13} aria-hidden />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function BodyEditor({
  body,
  onChange,
}: {
  body: BodySpec | null;
  onChange: (body: BodySpec | null) => void;
}) {
  if (!body) {
    return (
      <div className="param-table-wrap">
        <p className="param-table-empty">У запроса нет тела.</p>
        <div className="param-table-actions">
          <button
            type="button"
            className="artifact-btn"
            onClick={() =>
              onChange({ mediaType: "application/json", sample: "", params: [] })
            }
          >
            <Plus size={13} aria-hidden />
            Добавить тело запроса
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="body-editor">
      <div className="http-builder-line">
        <input
          className="http-builder-input"
          value={body.mediaType}
          aria-label="Content-Type"
          onChange={(e) => onChange({ ...body, mediaType: e.target.value })}
        />
        <button type="button" className="artifact-btn" onClick={() => onChange(null)}>
          <Trash2 size={13} aria-hidden />
          Убрать тело
        </button>
      </div>
      <SampleEditor
        label="Пример тела запроса"
        value={body.sample}
        onChange={(sample) => onChange({ ...body, sample })}
      />
      <ParamTable
        params={body.params}
        onChange={(params) => onChange({ ...body, params })}
        emptyHint="Поля тела запроса. Можно разобрать их из примера выше."
        onInferFromJson={() =>
          onChange({ ...body, params: mergeInferredParams(body.params, inferParamsFromJson(body.sample)) })
        }
      />
    </div>
  );
}

function ResponsesEditor({
  responses,
  onChange,
}: {
  responses: ResponseSpec[];
  onChange: (responses: ResponseSpec[]) => void;
}) {
  const update = (index: number, changes: Partial<ResponseSpec>) =>
    onChange(responses.map((r, i) => (i === index ? { ...r, ...changes } : r)));

  return (
    <div className="responses-editor">
      <div className="param-table-actions">
        <button
          type="button"
          className="artifact-btn"
          onClick={() =>
            onChange([...responses, { status: "200", description: "", sample: "", params: [] }])
          }
        >
          <Plus size={13} aria-hidden />
          Добавить ответ
        </button>
      </div>

      {responses.length === 0 ? (
        <p className="param-table-empty">
          Ответы сервиса — как минимум успешный. Из примера можно разобрать выходные параметры.
        </p>
      ) : null}

      {responses.map((response, index) => (
        <section key={index} className="response-card">
          <div className="http-builder-line">
            <input
              className="http-builder-input response-card-status"
              value={response.status}
              placeholder="200"
              aria-label={`Код ответа ${index + 1}`}
              onChange={(e) => update(index, { status: e.target.value })}
            />
            <input
              className="http-builder-input"
              value={response.description}
              placeholder="Описание ответа"
              aria-label={`Описание ответа ${index + 1}`}
              onChange={(e) => update(index, { description: e.target.value })}
            />
            <button
              type="button"
              className="param-table-remove"
              aria-label={`Удалить ответ ${index + 1}`}
              onClick={() => onChange(responses.filter((_, i) => i !== index))}
            >
              <Trash2 size={13} aria-hidden />
            </button>
          </div>
          <SampleEditor
            label="Пример ответа"
            value={response.sample}
            onChange={(sample) => update(index, { sample })}
          />
          <ParamTable
            params={response.params}
            onChange={(params) => update(index, { params })}
            emptyHint="Поля ответа. Можно разобрать их из примера выше."
            onInferFromJson={() =>
              update(index, {
                params: mergeInferredParams(response.params, inferParamsFromJson(response.sample)),
              })
            }
          />
        </section>
      ))}
    </div>
  );
}

function ErrorsEditor({
  errors,
  onChange,
}: {
  errors: ErrorSpec[];
  onChange: (errors: ErrorSpec[]) => void;
}) {
  const update = (index: number, changes: Partial<ErrorSpec>) =>
    onChange(errors.map((e, i) => (i === index ? { ...e, ...changes } : e)));

  return (
    <div className="param-table-wrap">
      <div className="param-table-actions">
        <button
          type="button"
          className="artifact-btn"
          onClick={() => onChange([...errors, { code: "", description: "" }])}
        >
          <Plus size={13} aria-hidden />
          Добавить ошибку
        </button>
      </div>
      {errors.length === 0 ? (
        <p className="param-table-empty">Коды ошибок, которые метод может вернуть.</p>
      ) : (
        <table className="param-table">
          <thead>
            <tr>
              <th>Код</th>
              <th>Описание</th>
              <th aria-label="Удалить" />
            </tr>
          </thead>
          <tbody>
            {errors.map((error, index) => (
              <tr key={index}>
                <td>
                  <input
                    value={error.code}
                    aria-label={`Код ошибки ${index + 1}`}
                    onChange={(e) => update(index, { code: e.target.value })}
                  />
                </td>
                <td>
                  <input
                    value={error.description}
                    aria-label={`Описание ошибки ${index + 1}`}
                    onChange={(e) => update(index, { description: e.target.value })}
                  />
                </td>
                <td>
                  <button
                    type="button"
                    className="param-table-remove"
                    aria-label={`Удалить ошибку ${index + 1}`}
                    onClick={() => onChange(errors.filter((_, i) => i !== index))}
                  >
                    <Trash2 size={13} aria-hidden />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function SampleEditor({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="sample-editor">
      <span className="sample-editor-label">{label}</span>
      <textarea
        className="sample-editor-input"
        rows={8}
        spellCheck={false}
        placeholder={'{\n  "type": "INVOICE"\n}'}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}
