import { ExternalLink, FileCode2, Lock } from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";
import type { OpenApiExplorerState } from "../../hooks/useOpenApiExplorerState";
import { operationKey } from "../../hooks/useOpenApiExplorerState";
import {
  asObject,
  effectiveParameters,
  effectiveServers,
  externalDocsOf,
  isRefMarker,
  namedExamples,
  type JsonValue,
} from "./openApiModel";
import { JsonExample } from "./JsonExample";
import { SchemaViewer, SchemaTypeInline } from "./SchemaViewer";
import { TryItOut } from "./TryItOut";
import { resolveOperationSecurity, type SecurityScheme } from "./security";
import type { SpecSourceTarget } from "./OpenApiExplorer";
import "./OpenApiExplorer.css";

type OperationViewProps = {
  path: string;
  method: string;
  operation: JsonValue;
  document: JsonValue;
  securitySchemes: SecurityScheme[];
  state: OpenApiExplorerState;
  /** Файл, из которого операция попала в сборку; `null` — источник неизвестен. */
  sourceFile: string | null;
  onOpenSource?: (target: SpecSourceTarget) => void;
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
  state,
  sourceFile,
  onOpenSource,
}: OperationViewProps) {
  const key = operationKey(path, method);
  const tryItOutOpen = state.tryOpen.has(key);
  const summary = typeof operation.summary === "string" ? operation.summary : null;
  const description =
    typeof operation.description === "string" ? operation.description : null;
  const operationId =
    typeof operation.operationId === "string" ? operation.operationId : null;
  const deprecated = operation.deprecated === true;

  const security = resolveOperationSecurity(document, operation);
  const parameters = effectiveParameters(document, path, operation);
  const externalDocs = externalDocsOf(operation);
  // Серверы показываем отдельной строкой только когда операция или её path
  // item их переопределяют — иначе это просто повтор корневого списка.
  const operationServers = effectiveServers(document, path, operation);
  const rootServers = effectiveServers(document, path, null);
  const overridesServers =
    operationServers.length > 0 &&
    JSON.stringify(operationServers) !== JSON.stringify(rootServers);

  const requestBody = asObject(operation.requestBody);
  const responses = asObject(operation.responses);

  return (
    <div className="oas-op">
      <div className="oas-op-header">
        <span className={`oas-method-badge oas-method-${method}`}>{method}</span>
        <span className={`oas-op-path${deprecated ? " is-deprecated" : ""}`}>{path}</span>
        {sourceFile && onOpenSource ? (
          <button
            type="button"
            className="oas-op-source-btn"
            onClick={() =>
              onOpenSource({
                file: sourceFile,
                searchKeys: [operationId, path].filter((k): k is string => Boolean(k)),
              })
            }
            title={`Открыть исходник: ${sourceFile}`}
          >
            <FileCode2 size={13} aria-hidden />
            Исходник
          </button>
        ) : null}
        <button
          type="button"
          className={`oas-try-toggle ${tryItOutOpen ? "active" : ""}`}
          onClick={() => state.toggleTryOpen(key)}
        >
          {tryItOutOpen ? "Отмена" : "Try it out"}
        </button>
      </div>
      {deprecated ? (
        <div className="oas-op-deprecated">
          Операция помечена как deprecated — не используйте её в новых интеграциях.
        </div>
      ) : null}
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
      {externalDocs ? (
        <button
          type="button"
          className="oas-op-external"
          onClick={() => void openPath(externalDocs.url).catch(() => {})}
          title={externalDocs.url}
        >
          <ExternalLink size={12} aria-hidden />
          {externalDocs.description ?? "Внешняя документация"}
        </button>
      ) : null}
      {overridesServers ? (
        <div className="oas-op-servers">
          Свои серверы:{" "}
          {operationServers.map((server) => (
            <code key={server.url}>{server.url}</code>
          ))}
        </div>
      ) : null}

      {tryItOutOpen ? (
        <TryItOut
          path={path}
          method={method}
          operation={operation}
          document={document}
          securitySchemes={securitySchemes}
          state={state}
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
          {contentEntries(requestBody.content).map(([mediaType, mediaObj]) => (
            <MediaBlock key={mediaType} mediaType={mediaType} media={mediaObj} />
          ))}
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
                {entries.map(([mediaType, mediaObj]) => (
                  <MediaBlock key={mediaType} mediaType={mediaType} media={mediaObj} />
                ))}
              </div>
            );
          })}
        </section>
      ) : null}
    </div>
  );
}

/** Схема media type плюс объявленные в спеке именованные примеры. Сгенерённый
 * из схемы JSON остаётся: он показывает форму целиком, тогда как примеры —
 * конкретные случаи, которые автор счёл важными. */
function MediaBlock({ mediaType, media }: { mediaType: string; media: unknown }) {
  const mediaObj = asObject(media);
  const schema = mediaObj?.schema;
  const examples = mediaObj ? namedExamples(mediaObj) : [];

  if (schema === undefined && examples.length === 0) return null;

  return (
    <div className="oas-media-block">
      <div className="oas-media-type">{mediaType}</div>
      {schema !== undefined ? (
        <>
          <SchemaViewer schema={schema} />
          <JsonExample schema={schema} />
        </>
      ) : null}
      {examples.map((example) => (
        <JsonExample
          key={example.name}
          value={example.value}
          title={`Пример: ${example.summary ?? example.name}`}
        />
      ))}
    </div>
  );
}
