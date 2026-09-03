import { useMemo, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { toMessage } from "../../lib/errors";
import type { OpenApiExplorerState, TryItOutForm } from "../../hooks/useOpenApiExplorerState";
import { operationKey } from "../../hooks/useOpenApiExplorerState";
import {
  effectiveParameters,
  effectiveServers,
  primaryRequestBodyMedia,
  type JsonValue,
} from "./openApiModel";
import {
  buildCurl,
  buildRequest,
  paramKey,
  scalarSkeleton,
  skeletonForSchema,
} from "./requestBuilder";
import {
  credentialsFor,
  isFilled,
  resolveOperationSecurity,
  type SecurityScheme,
} from "./security";
import { executeRequest } from "./requestExecutor";
import { ResponseBodyView } from "./ResponseBodyView";
import { ServerSelect } from "./ServerSelect";
import "./OpenApiExplorer.css";

type TryItOutProps = {
  path: string;
  method: string;
  operation: JsonValue;
  document: JsonValue;
  securitySchemes: SecurityScheme[];
  state: OpenApiExplorerState;
};

export function TryItOut({
  path,
  method,
  operation,
  document,
  securitySchemes,
  state,
}: TryItOutProps) {
  const key = operationKey(path, method);
  const parameters = useMemo(
    () => effectiveParameters(document, path, operation),
    [document, path, operation],
  );
  const bodyMedia = useMemo(() => primaryRequestBodyMedia(operation), [operation]);
  const serverUrls = useMemo(
    () => effectiveServers(document, path, operation).map((s) => s.url),
    [document, path, operation],
  );
  const hasBody = bodyMedia !== null;

  const [executing, setExecuting] = useState(false);
  const [copied, setCopied] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);

  // Форма живёт в состоянии Explorer'а: возврат к операции (или закрытие и
  // повторное открытие вкладки) не должен стирать уже введённые значения.
  // Первое открытие получает скелет, посчитанный из схем.
  const initialForm = useMemo<TryItOutForm>(() => {
    const paramValues: TryItOutForm["paramValues"] = {};
    for (const p of parameters) {
      paramValues[paramKey(p.in, p.name)] = scalarSkeleton(p.schema);
    }
    return {
      paramValues,
      bodyText:
        bodyMedia?.schema !== undefined
          ? JSON.stringify(skeletonForSchema(bodyMedia.schema), null, 2)
          : "",
    };
  }, [parameters, bodyMedia]);

  const form = state.forms[key] ?? initialForm;
  const baseUrl = state.baseUrlOverride ?? serverUrls[0] ?? "";

  const security = useMemo(
    () => resolveOperationSecurity(document, operation),
    [document, operation],
  );
  const credentials = useMemo(
    () => credentialsFor(securitySchemes, state.authValues, security.schemeIds),
    [securitySchemes, state.authValues, security.schemeIds],
  );
  const missingSchemeIds = security.schemeIds.filter(
    (id) => !isFilled(state.authValues[id]),
  );

  const request = useMemo(
    () =>
      buildRequest({
        baseUrl,
        path,
        method,
        paramValues: form.paramValues,
        paramEntries: parameters,
        bodyMediaType: bodyMedia?.mediaType ?? null,
        bodyText: form.bodyText,
        hasBody,
        auth: credentials,
      }),
    [baseUrl, path, method, form, parameters, bodyMedia, hasBody, credentials],
  );

  const curl = useMemo(() => buildCurl(request), [request]);

  const runs = state.history[key] ?? [];
  const latest = runs[0] ?? null;

  const setParam = (location: string, name: string, value: string) => {
    state.setForm(key, {
      ...form,
      paramValues: { ...form.paramValues, [paramKey(location, name)]: value },
    });
  };

  const handleExecute = async () => {
    setExecuting(true);
    try {
      const result = await executeRequest(request);
      state.pushRun(key, {
        at: Date.now(),
        method: request.method,
        url: request.url,
        response: result,
        error: null,
      });
    } catch (e) {
      state.pushRun(key, {
        at: Date.now(),
        method: request.method,
        url: request.url,
        response: null,
        error: toMessage(e),
      });
    } finally {
      setExecuting(false);
    }
  };

  const handleCopyCurl = async () => {
    try {
      await writeText(curl);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard unavailable — silently ignore, curl text is still visible.
    }
  };

  const groups: Record<string, typeof parameters> = {
    path: [],
    query: [],
    header: [],
  };
  for (const p of parameters) {
    (groups[p.in] ?? (groups[p.in] = [])).push(p);
  }

  const statusClassOf = (status: number) =>
    status < 300
      ? "oas-try-status-ok"
      : status < 500
        ? "oas-try-status-warn"
        : "oas-try-status-error";

  return (
    <div className="oas-try">
      <div className="oas-try-group">
        <div className="oas-try-group-title">Сервер</div>
        <div className="oas-try-server-stack">
          <div className="oas-try-server-field">
            <span className="oas-try-server-field-label">Выбрать из спецификации</span>
            <ServerSelect
              servers={serverUrls}
              value={baseUrl}
              onSelect={state.setBaseUrlOverride}
            />
          </div>
          <div className="oas-try-server-field">
            <span className="oas-try-server-field-label">Итоговый адрес запроса</span>
            <input
              type="text"
              className="oas-try-input"
              value={baseUrl}
              onChange={(e) => state.setBaseUrlOverride(e.target.value)}
              placeholder="https://host"
            />
          </div>
        </div>
      </div>

      {security.declared ? (
        <div className="oas-try-group">
          <div className="oas-try-group-title">Авторизация</div>
          {credentials.length > 0 ? (
            <ul className="oas-try-auth-list">
              {credentials.map((credential) => (
                <li key={`${credential.in}:${credential.name}`}>
                  <code>{credential.name}</code>
                  <span>
                    {credential.in === "header"
                      ? " — заголовок подставлен"
                      : " — query-параметр подставлен"}
                  </span>
                </li>
              ))}
            </ul>
          ) : null}
          {missingSchemeIds.length > 0 ? (
            <div
              className={`oas-try-auth-missing${security.optional ? "" : " is-required"}`}
            >
              <span>
                {security.optional ? "Не заполнено" : "Требуется, но не заполнено"}:{" "}
                {missingSchemeIds.join(", ")} — запрос уйдёт без этих данных.
              </span>
              <button
                type="button"
                className="oas-try-copy-btn"
                onClick={() => state.setAuthOpen(true)}
              >
                Заполнить
              </button>
            </div>
          ) : null}
        </div>
      ) : null}

      {(["path", "query", "header"] as const).map((location) =>
        groups[location].length > 0 ? (
          <div key={location} className="oas-try-group">
            <div className="oas-try-group-title">
              {location === "path" ? "Path" : location === "query" ? "Query" : "Headers"}
            </div>
            {groups[location].map((p) => (
              <div key={p.name} className="oas-try-row">
                <label className="oas-try-label">
                  {p.name}
                  {p.required ? <span className="oas-schema-required">*</span> : null}
                </label>
                <input
                  type="text"
                  className="oas-try-input"
                  value={form.paramValues[paramKey(p.in, p.name)] ?? ""}
                  onChange={(e) => setParam(p.in, p.name, e.target.value)}
                  placeholder={p.description ?? ""}
                />
              </div>
            ))}
          </div>
        ) : null,
      )}

      {hasBody ? (
        <div className="oas-try-group">
          <div className="oas-try-group-title">Тело запроса ({bodyMedia?.mediaType})</div>
          <textarea
            className="oas-try-body"
            value={form.bodyText}
            onChange={(e) => state.setForm(key, { ...form, bodyText: e.target.value })}
            spellCheck={false}
            rows={10}
          />
        </div>
      ) : null}

      <div className="oas-try-group">
        <div className="oas-try-group-title">Curl</div>
        <pre className="oas-try-curl">{curl}</pre>
      </div>

      <div className="oas-try-actions">
        <button
          type="button"
          className="oas-try-execute-btn"
          onClick={() => void handleExecute()}
          disabled={executing || !baseUrl}
        >
          {executing ? "Выполняется…" : "Выполнить"}
        </button>
        <button type="button" className="oas-try-copy-btn" onClick={() => void handleCopyCurl()}>
          {copied ? "Скопировано" : "Копировать"}
        </button>
        {runs.length > 1 ? (
          <button
            type="button"
            className="oas-try-copy-btn"
            onClick={() => setHistoryOpen((o) => !o)}
          >
            {historyOpen ? "Скрыть историю" : `История (${runs.length})`}
          </button>
        ) : null}
      </div>

      {latest?.error ? (
        <div className="oas-try-error">{latest.error}</div>
      ) : latest?.response ? (
        <div className="oas-try-response">
          <div className="oas-try-response-header">
            <span className={`oas-try-status ${statusClassOf(latest.response.status)}`}>
              {latest.response.status} {latest.response.statusText}
            </span>
            <span className="oas-try-duration">
              {Math.round(latest.response.durationMs)} мс
            </span>
          </div>
          {Object.keys(latest.response.headers).length > 0 ? (
            <details className="oas-try-headers">
              <summary>
                Заголовки ответа ({Object.keys(latest.response.headers).length})
              </summary>
              <table className="oas-try-headers-table">
                <tbody>
                  {Object.entries(latest.response.headers).map(([k, v]) => (
                    <tr key={k}>
                      <td className="oas-try-header-name">{k}</td>
                      <td>{v}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </details>
          ) : null}
          <ResponseBodyView body={latest.response.body} />
        </div>
      ) : null}

      {historyOpen && runs.length > 1 ? (
        <div className="oas-try-history">
          <div className="oas-try-group-title">Предыдущие запуски</div>
          {runs.slice(1).map((run) => (
            <div key={run.at} className="oas-try-history-row">
              <span className="oas-try-history-time">
                {new Date(run.at).toLocaleTimeString()}
              </span>
              {run.response ? (
                <span className={`oas-try-status ${statusClassOf(run.response.status)}`}>
                  {run.response.status}
                </span>
              ) : (
                <span className="oas-try-status oas-try-status-error">ошибка</span>
              )}
              <span className="oas-try-history-url" title={run.url}>
                {run.url}
              </span>
              {run.response ? (
                <span className="oas-try-duration">
                  {Math.round(run.response.durationMs)} мс
                </span>
              ) : null}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
