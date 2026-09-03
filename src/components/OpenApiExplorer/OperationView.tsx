import { useState } from "react";
import { Lock } from "lucide-react";
import { asObject, isRefMarker, parseParameters, type JsonValue } from "./openApiModel";
import { JsonExample } from "./JsonExample";
import { SchemaViewer, SchemaTypeInline } from "./SchemaViewer";
import { TryItOut } from "./TryItOut";
import {
  resolveOperationSecurity,
  type AuthValues,
  type SecurityScheme,
} from "./security";
import "./OpenApiExplorer.css";

type OperationViewProps = {
  path: string;
  method: string;
  operation: JsonValue;
  document: JsonValue;
  securitySchemes: SecurityScheme[];
  authValues: AuthValues;
  /** Раскрыть панель авторизации — из подсказки «токен не заполнен». */
  onRequestAuth: () => void;
};

function contentEntries(content: unknown): [string, unknown][] {
  const obj = asObject(content);
  if (!obj) return [];
  return Object.entries(obj);
}

export function OperationView({
  path,
  method,
  operation,
  document,
  securitySchemes,
  authValues,
  onRequestAuth,
}: OperationViewProps) {
  const [tryItOutOpen, setTryItOutOpen] = useState(false);
  const security = resolveOperationSecurity(document, operation);
  const summary = typeof operation.summary === "string" ? operation.summary : null;
  const description =
    typeof operation.description === "string" ? operation.description : null;
  const operationId =
    typeof operation.operationId === "string" ? operation.operationId : null;

  const parameters = parseParameters(operation);

  const requestBody = asObject(operation.requestBody);
  const responses = asObject(operation.responses);

  return (
    <div className="oas-op">
      <div className="oas-op-header">
        <span className={`oas-method-badge oas-method-${method}`}>{method}</span>
        <span className="oas-op-path">{path}</span>
        <button
          type="button"
          className={`oas-try-toggle ${tryItOutOpen ? "active" : ""}`}
          onClick={() => setTryItOutOpen((o) => !o)}
        >
          {tryItOutOpen ? "Отмена" : "Try it out"}
        </button>
      </div>
      {summary ? <div className="oas-op-summary">{summary}</div> : null}
      {security.declared ? (
        <div className="oas-op-security">
          <Lock size={12} aria-hidden />
          <span>
            {security.optional ? "Авторизация необязательна: " : "Требует авторизации: "}
            {security.schemeIds.join(", ")}
          </span>
        </div>
      ) : null}
      {operationId ? <div className="oas-op-id">operationId: {operationId}</div> : null}
      {description ? <p className="oas-op-description">{description}</p> : null}

      {tryItOutOpen ? (
        <TryItOut
          path={path}
          method={method}
          operation={operation}
          document={document}
          securitySchemes={securitySchemes}
          authValues={authValues}
          onRequestAuth={onRequestAuth}
        />
      ) : null}

      {parameters.length > 0 ? (
        <section className="oas-section">
          <h3 className="oas-section-title">Параметры</h3>
          <table className="oas-params-table">
            <thead>
              <tr>
                <th>Имя</th>
                <th>В</th>
                <th>Тип</th>
                <th>Обязателен</th>
                <th>Описание</th>
              </tr>
            </thead>
            <tbody>
              {parameters.map((p, i) => (
                <tr key={`${p.in}-${p.name}-${i}`}>
                  <td className="oas-param-name">{p.name}</td>
                  <td>{p.in}</td>
                  <td>
                    <SchemaTypeInline schema={p.schema} />
                  </td>
                  <td>{p.required ? "да" : "нет"}</td>
                  <td className="oas-param-desc">{p.description ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ) : null}

      {requestBody ? (
        <section className="oas-section">
          <h3 className="oas-section-title">
            Тело запроса{requestBody.required ? " (обязательно)" : ""}
          </h3>
          {contentEntries(requestBody.content).map(([mediaType, mediaObj]) => {
            const schema = asObject(mediaObj)?.schema;
            return (
              <div key={mediaType} className="oas-media-block">
                <div className="oas-media-type">{mediaType}</div>
                <SchemaViewer schema={schema} />
                <JsonExample schema={schema} />
              </div>
            );
          })}
        </section>
      ) : null}

      {responses ? (
        <section className="oas-section">
          <h3 className="oas-section-title">Ответы</h3>
          {Object.entries(responses).map(([status, respRaw]) => {
            if (isRefMarker(respRaw)) {
              return (
                <div key={status} className="oas-response-block">
                  <div className="oas-response-status">{status}</div>
                  <SchemaViewer schema={respRaw} />
                </div>
              );
            }
            const resp = asObject(respRaw);
            if (!resp) return null;
            const respDescription =
              typeof resp.description === "string" ? resp.description : null;
            const entries = contentEntries(resp.content);
            return (
              <div key={status} className="oas-response-block">
                <div className="oas-response-status">
                  <span className={`oas-status-badge oas-status-${status[0]}xx`}>
                    {status}
                  </span>
                  {respDescription ? (
                    <span className="oas-response-desc">{respDescription}</span>
                  ) : null}
                </div>
                {entries.map(([mediaType, mediaObj]) => {
                  const schema = asObject(mediaObj)?.schema;
                  if (schema === undefined) return null;
                  return (
                    <div key={mediaType} className="oas-media-block">
                      <div className="oas-media-type">{mediaType}</div>
                      <SchemaViewer schema={schema} />
                      <JsonExample schema={schema} />
                    </div>
                  );
                })}
              </div>
            );
          })}
        </section>
      ) : null}
    </div>
  );
}
