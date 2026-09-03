import { useEffect, useMemo } from "react";
import { ChevronDown, ChevronRight, Download, Lock } from "lucide-react";
import type { OpenApiBundleResult } from "../../lib/openapi";
import type { OpenApiExplorerState } from "../../hooks/useOpenApiExplorerState";
import {
  collectOperations,
  getOperation,
  groupByTag,
  matchesFilter,
  type JsonValue,
  type OperationSummary,
} from "./openApiModel";
import { OperationView } from "./OperationView";
import { AuthPanel } from "./AuthPanel";
import { collectSecuritySchemes, resolveOperationSecurity } from "./security";
import { buildSourceIndex, operationPointer, sourceForPointer } from "./sourceMap";
import "./OpenApiExplorer.css";

export type SpecSourceTarget = {
  /** Путь файла относительно корня репозитория. */
  file: string;
  /** Подсказки для поиска строки внутри файла, от точной к грубой. */
  searchKeys: string[];
};

type OpenApiExplorerProps = {
  bundle: OpenApiBundleResult | null;
  loading: boolean;
  error: string | null;
  state: OpenApiExplorerState;
  /** Открыть файл-исходник (в редакторе, с прокруткой к нужной строке). */
  onOpenSource?: (target: SpecSourceTarget) => void;
  /** Запрос «показать операцию из этого файла» — приходит из полосы вкладок,
   * когда открыт фрагмент спеки. `nonce` делает повторный запрос заметным. */
  revealSource?: { file: string; nonce: number } | null;
  /** Сохранить собранный документ в файл. */
  onExportBundle?: (json: string) => void;
};

function opKey(op: OperationSummary): string {
  return `${op.method} ${op.path}`;
}

export function OpenApiExplorer({
  bundle,
  loading,
  error,
  state,
  onOpenSource,
  revealSource = null,
  onExportBundle,
}: OpenApiExplorerProps) {
  const { filter, setFilter, selected, setSelected } = state;

  const document = bundle?.document as JsonValue | undefined;

  const operations = useMemo(
    () => (document ? collectOperations(document) : []),
    [document],
  );
  const filtered = useMemo(
    () => operations.filter((op) => matchesFilter(op, filter)),
    [operations, filter],
  );
  const grouped = useMemo(() => groupByTag(filtered), [filtered]);
  const securitySchemes = useMemo(
    () => (document ? collectSecuritySchemes(document) : []),
    [document],
  );
  const sourceIndex = useMemo(
    () => buildSourceIndex(bundle?.sources ?? []),
    [bundle?.sources],
  );
  // Замочек в списке считаем один раз на документ: `security` может прийти и
  // от корня спеки, так что по самой операции этого не видно.
  const securedOperations = useMemo(() => {
    if (!document) return new Set<string>();
    const secured = new Set<string>();
    for (const op of operations) {
      const target = getOperation(document, op.path, op.method);
      if (target && resolveOperationSecurity(document, target).declared) {
        secured.add(opKey(op));
      }
    }
    return secured;
  }, [document, operations]);

  /** Файл-исходник операции — для «показать в Explorer» из полосы вкладок и
   * для подписи находок валидации. */
  const sourceFileOfOperation = useMemo(() => {
    const map = new Map<string, string>();
    for (const op of operations) {
      const source = sourceForPointer(sourceIndex, operationPointer(op.path, op.method));
      if (source) map.set(opKey(op), source.file);
    }
    return map;
  }, [operations, sourceIndex]);

  // Обратное направление моста «исходник ↔ рендер»: пользователь стоит в
  // `operations/listPets.yaml` и просит показать эту ручку в рендере.
  useEffect(() => {
    if (!revealSource) return;
    const match = operations.find(
      (op) => sourceFileOfOperation.get(opKey(op)) === revealSource.file,
    );
    if (match) setSelected({ path: match.path, method: match.method });
  }, [revealSource, operations, sourceFileOfOperation, setSelected]);

  // `selected` can point at an operation that no longer exists after the
  // spec reloads (endpoint deleted from disk) — falls back to the first
  // remaining operation exactly like an unset selection does, instead of
  // rendering a stale nav highlight next to a blank detail pane.
  const selectedOperation =
    document && selected ? getOperation(document, selected.path, selected.method) : null;

  const activeSelection =
    selected && selectedOperation
      ? selected
      : filtered.length > 0
        ? { path: filtered[0].path, method: filtered[0].method }
        : null;

  const activeOperation =
    selectedOperation ??
    (document && activeSelection
      ? getOperation(document, activeSelection.path, activeSelection.method)
      : null);

  if (loading) {
    return <div className="oas-explorer panel-empty">Загрузка спецификации…</div>;
  }
  if (error) {
    return <div className="oas-explorer panel-empty">{error}</div>;
  }
  if (!document) {
    return <div className="oas-explorer panel-empty">Нет содержимого</div>;
  }

  const info = (document.info as JsonValue | undefined) ?? {};
  const title = typeof info.title === "string" ? info.title : "API";
  const version = typeof info.version === "string" ? info.version : null;
  const description = typeof info.description === "string" ? info.description : null;
  const servers = Array.isArray(document.servers) ? document.servers : [];

  const allTags = [...grouped.keys()];

  return (
    <div className="oas-explorer">
      <div className="oas-nav">
        <div className="oas-nav-header">
          <div className="oas-nav-title">{title}</div>
          {version ? <div className="oas-nav-version">v{version}</div> : null}
        </div>
        <input
          type="text"
          className="oas-nav-filter"
          placeholder="Поиск операций…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <div className="oas-nav-actions">
          <span className="oas-nav-count">операций: {filtered.length}</span>
          <button
            type="button"
            className="oas-nav-action"
            onClick={() =>
              state.setAllTagsCollapsed(allTags, state.collapsedTags.size < allTags.length)
            }
          >
            {state.collapsedTags.size < allTags.length ? "Свернуть все" : "Развернуть все"}
          </button>
        </div>
        <div className="oas-nav-groups">
          {[...grouped.entries()].map(([tag, ops]) => {
            const collapsed = state.collapsedTags.has(tag);
            return (
              <div key={tag} className="oas-nav-group">
                <button
                  type="button"
                  className="oas-nav-group-title"
                  onClick={() => state.toggleTag(tag)}
                  aria-expanded={!collapsed}
                >
                  {collapsed ? (
                    <ChevronRight size={12} aria-hidden />
                  ) : (
                    <ChevronDown size={12} aria-hidden />
                  )}
                  {tag}
                  <span className="oas-nav-group-count">{ops.length}</span>
                </button>
                {collapsed
                  ? null
                  : ops.map((op) => {
                      const isActive =
                        activeSelection?.path === op.path &&
                        activeSelection?.method === op.method;
                      return (
                        <button
                          key={opKey(op)}
                          type="button"
                          className={`oas-nav-item ${isActive ? "active" : ""}${
                            op.deprecated ? " is-deprecated" : ""
                          }`}
                          onClick={() => setSelected({ path: op.path, method: op.method })}
                          title={op.summary ?? op.path}
                        >
                          <span className={`oas-method-badge oas-method-${op.method}`}>
                            {op.method}
                          </span>
                          <span className="oas-nav-item-text">
                            <span className="oas-nav-item-path">{op.path}</span>
                            {op.summary ? (
                              <span className="oas-nav-item-summary">{op.summary}</span>
                            ) : null}
                          </span>
                          {securedOperations.has(opKey(op)) ? (
                            <Lock
                              size={11}
                              className="oas-nav-item-lock"
                              aria-label="требует авторизации"
                            />
                          ) : null}
                        </button>
                      );
                    })}
              </div>
            );
          })}
          {grouped.size === 0 ? (
            <div className="panel-empty">Ничего не найдено</div>
          ) : null}
        </div>
      </div>
      <div className="oas-main">
        <div className="oas-main-head">
          <AuthPanel
            schemes={securitySchemes}
            values={state.authValues}
            onChange={state.setAuthValue}
            onClear={state.clearAuth}
            open={state.authOpen}
            onToggle={() => state.setAuthOpen(!state.authOpen)}
            activeSchemeIds={
              activeOperation ? resolveOperationSecurity(document, activeOperation).schemeIds : []
            }
            trailing={
              onExportBundle ? (
                <button
                  type="button"
                  className="oas-auth-clear"
                  onClick={() => onExportBundle(JSON.stringify(document, null, 2))}
                  title="Сохранить собранную спецификацию одним файлом"
                >
                  <Download size={12} aria-hidden /> Экспорт
                </button>
              ) : null
            }
          />
        </div>
        {description ? <p className="oas-info-description">{description}</p> : null}
        {servers.length > 0 && !activeOperation ? (
          <div className="oas-servers">
            <h3 className="oas-section-title">Серверы</h3>
            <ul>
              {servers.map((s, i) => {
                const server = s as JsonValue;
                return (
                  <li key={i}>
                    <code>{String(server.url)}</code>
                    {typeof server.description === "string" ? ` — ${server.description}` : ""}
                  </li>
                );
              })}
            </ul>
          </div>
        ) : null}
        {activeSelection && activeOperation ? (
          <OperationView
            path={activeSelection.path}
            method={activeSelection.method}
            operation={activeOperation}
            document={document}
            securitySchemes={securitySchemes}
            state={state}
            sourceFile={
              sourceForPointer(
                sourceIndex,
                operationPointer(activeSelection.path, activeSelection.method),
              )?.file ?? null
            }
            onOpenSource={onOpenSource}
          />
        ) : operations.length === 0 ? (
          <div className="panel-empty">В спецификации нет операций</div>
        ) : null}
      </div>
    </div>
  );
}
