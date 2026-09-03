import { useEffect, useMemo, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { toMessage } from "../../lib/errors";
import { parseParameters, primaryRequestBodyMedia, type JsonValue } from "./openApiModel";
import {
  buildCurl,
  buildRequest,
  listServerUrls,
  paramKey,
  scalarSkeleton,
  skeletonForSchema,
  type ParamValues,
} from "./requestBuilder";
import {
  credentialsFor,
  isFilled,
  resolveOperationSecurity,
  type AuthValues,
  type SecurityScheme,
} from "./security";
import { executeRequest, type ExecutedResponse } from "./requestExecutor";
import { ResponseBodyView } from "./ResponseBodyView";
import { ServerSelect } from "./ServerSelect";
import "./OpenApiExplorer.css";

type TryItOutProps = {
  path: string;
  method: string;
  operation: JsonValue;
  document: JsonValue;
  securitySchemes: SecurityScheme[];
  authValues: AuthValues;
  onRequestAuth: () => void;
};

export function TryItOut({
  path,
  method,
  operation,
  document,
  securitySchemes,
  authValues,
  onRequestAuth,
}: TryItOutProps) {
  const parameters = useMemo(() => parseParameters(operation), [operation]);
  const bodyMedia = useMemo(() => primaryRequestBodyMedia(operation), [operation]);
  const serverUrls = useMemo(() => listServerUrls(document), [document]);
  const hasBody = bodyMedia !== null;

  const security = useMemo(
    () => resolveOperationSecurity(document, operation),
    [document, operation],
  );
  const credentials = useMemo(
    () => credentialsFor(securitySchemes, authValues, security.schemeIds),
    [securitySchemes, authValues, security.schemeIds],
  );
  const missingSchemeIds = security.schemeIds.filter((id) => !isFilled(authValues[id]));

  const [baseUrl, setBaseUrl] = useState(serverUrls[0] ?? "");
  const [paramValues, setParamValues] = useState<ParamValues>({});
  const [bodyText, setBodyText] = useState("");
  const [executing, setExecuting] = useState(false);
  const [response, setResponse] = useState<ExecutedResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  // Reset the form whenever the selected operation changes.
  useEffect(() => {
    setBaseUrl(serverUrls[0] ?? "");
    const initial: ParamValues = {};
    for (const p of parameters) {
      initial[paramKey(p.in, p.name)] = scalarSkeleton(p.schema);
    }
    setParamValues(initial);
    setBodyText(
      bodyMedia?.schema !== undefined
        ? JSON.stringify(skeletonForSchema(bodyMedia.schema), null, 2)
        : "",
    );
    setResponse(null);
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, method]);

  const request = useMemo(
    () =>
      buildRequest({
        baseUrl,
        path,
        method,
        paramValues,
        paramEntries: parameters,
        bodyMediaType: bodyMedia?.mediaType ?? null,
        bodyText,
        hasBody,
        auth: credentials,
      }),
    [baseUrl, path, method, paramValues, parameters, bodyMedia, bodyText, hasBody, credentials],
  );

  const curl = useMemo(() => buildCurl(request), [request]);

  const setParam = (location: string, name: string, value: string) => {
    setParamValues((prev) => ({ ...prev, [paramKey(location, name)]: value }));
  };

  const handleExecute = async () => {
    setExecuting(true);
    setError(null);
    try {
      const result = await executeRequest(request);
      setResponse(result);
    } catch (e) {
      setResponse(null);
      setError(toMessage(e));
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

  const statusClass = response
    ? response.status < 300
      ? "oas-try-status-ok"
      : response.status < 500
        ? "oas-try-status-warn"
        : "oas-try-status-error"
    : "";

  return (
    <div className="oas-try">
      <div className="oas-try-group">
        <div className="oas-try-group-title">Сервер</div>
        <div className="oas-try-server-stack">
          <div className="oas-try-server-field">
            <span className="oas-try-server-field-label">Выбрать из спецификации</span>
            <ServerSelect servers={serverUrls} value={baseUrl} onSelect={setBaseUrl} />
          </div>
          <div className="oas-try-server-field">
            <span className="oas-try-server-field-label">Итоговый адрес запроса</span>
            <input
              type="text"
              className="oas-try-input"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
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
              <button type="button" className="oas-try-copy-btn" onClick={onRequestAuth}>
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
                  value={paramValues[paramKey(p.in, p.name)] ?? ""}
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
            value={bodyText}
            onChange={(e) => setBodyText(e.target.value)}
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
      </div>

      {error ? (
        <div className="oas-try-error">{error}</div>
      ) : response ? (
        <div className="oas-try-response">
          <div className="oas-try-response-header">
            <span className={`oas-try-status ${statusClass}`}>
              {response.status} {response.statusText}
            </span>
            <span className="oas-try-duration">{Math.round(response.durationMs)} мс</span>
          </div>
          {Object.keys(response.headers).length > 0 ? (
            <details className="oas-try-headers">
              <summary>Заголовки ответа ({Object.keys(response.headers).length})</summary>
              <table className="oas-try-headers-table">
                <tbody>
                  {Object.entries(response.headers).map(([k, v]) => (
                    <tr key={k}>
                      <td className="oas-try-header-name">{k}</td>
                      <td>{v}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </details>
          ) : null}
          <ResponseBodyView body={response.body} />
        </div>
      ) : null}
    </div>
  );
}
