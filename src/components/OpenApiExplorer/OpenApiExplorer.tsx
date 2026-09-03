import { useCallback, useMemo, useState } from "react";
import { Lock } from "lucide-react";
import type { OpenApiBundleResult } from "../../lib/openapi";
import {
  collectOperations,
  getOperation,
  groupByTag,
  matchesFilter,
  type JsonValue,
  type OperationSummary,
} from "./openApiModel";
import { OperationView } from "./OperationView";
import { DiagnosticsBanner } from "./DiagnosticsBanner";
import { AuthPanel } from "./AuthPanel";
import {
  collectSecuritySchemes,
  resolveOperationSecurity,
  type AuthValue,
  type AuthValues,
} from "./security";
import "./OpenApiExplorer.css";

type OpenApiExplorerProps = {
  bundle: OpenApiBundleResult | null;
  loading: boolean;
  error: string | null;
};

function opKey(op: OperationSummary): string {
  return `${op.method} ${op.path}`;
}

export function OpenApiExplorer({ bundle, loading, error }: OpenApiExplorerProps) {
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<{ path: string; method: string } | null>(null);
  // Секреты — на всю спецификацию и только в памяти вкладки: на диск их не
  // пишем, а переключение операции не должно заставлять вводить токен заново.
  const [authValues, setAuthValues] = useState<AuthValues>({});
  const [authOpen, setAuthOpen] = useState(false);

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

  const setAuthValue = useCallback((schemeId: string, value: AuthValue) => {
    setAuthValues((prev) => ({ ...prev, [schemeId]: value }));
  }, []);
  const clearAuth = useCallback(() => setAuthValues({}), []);

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
        <div className="oas-nav-groups">
          {[...grouped.entries()].map(([tag, ops]) => (
            <div key={tag} className="oas-nav-group">
              <div className="oas-nav-group-title">{tag}</div>
              {ops.map((op) => {
                const isActive =
                  activeSelection?.path === op.path && activeSelection?.method === op.method;
                return (
                  <button
                    key={opKey(op)}
                    type="button"
                    className={`oas-nav-item ${isActive ? "active" : ""}`}
                    onClick={() => setSelected({ path: op.path, method: op.method })}
                  >
                    <span className={`oas-method-badge oas-method-${op.method}`}>
                      {op.method}
                    </span>
                    <span className="oas-nav-item-path">{op.path}</span>
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
          ))}
          {grouped.size === 0 ? (
            <div className="panel-empty">Ничего не найдено</div>
          ) : null}
        </div>
      </div>
      <div className="oas-main">
        <AuthPanel
          schemes={securitySchemes}
          values={authValues}
          onChange={setAuthValue}
          onClear={clearAuth}
          open={authOpen}
          onToggle={() => setAuthOpen((o) => !o)}
          activeSchemeIds={
            activeOperation ? resolveOperationSecurity(document, activeOperation).schemeIds : []
          }
        />
        {bundle && bundle.diagnostics.length > 0 ? (
          <DiagnosticsBanner diagnostics={bundle.diagnostics} />
        ) : null}
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
            authValues={authValues}
            onRequestAuth={() => setAuthOpen(true)}
          />
        ) : operations.length === 0 ? (
          <div className="panel-empty">В спецификации нет операций</div>
        ) : null}
      </div>
    </div>
  );
}
