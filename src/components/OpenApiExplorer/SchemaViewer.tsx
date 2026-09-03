import { useState } from "react";
import {
  isRefMarker,
  localSchemaRefName,
  type JsonValue,
  type RefMarker,
} from "./openApiModel";
import { compatibleExampleForSchema } from "./requestBuilder";
import "./OpenApiExplorer.css";

type SchemaViewerProps = {
  schema: unknown;
  name?: string;
  depth?: number;
  required?: boolean;
};

function asObject(value: unknown): JsonValue | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as JsonValue)
    : null;
}

function asSchemaArray(value: unknown): unknown[] | null {
  return Array.isArray(value) ? value : null;
}

function typeLabel(s: JsonValue): string {
  const type = s.type;
  const format = typeof s.format === "string" ? s.format : null;
  if (Array.isArray(type)) return type.join(" | ");
  if (typeof type === "string") return format ? `${type} (${format})` : type;
  if (s.allOf) return "allOf";
  if (s.oneOf) return "oneOf";
  if (s.anyOf) return "anyOf";
  if (s.properties) return "object";
  if (s.items) return "array";
  return "any";
}

function RefBadge({ marker }: { marker: RefMarker }) {
  const label = marker.circular ? "циклическая ссылка" : marker.reason ?? "не удалось разрешить";
  return (
    <span className="oas-ref-badge" title={marker.$ref}>
      ↻ {label}
    </span>
  );
}

/** Рекурсивная схема: сборщик вынес её тело в `components/schemas`, здесь
 * осталась ссылка. Это нормальная конструкция, а не поломка, поэтому и
 * выглядит она как ссылка на тип, а не как предупреждение. */
function RecursiveRefBadge({ name }: { name: string }) {
  return (
    <span
      className="oas-ref-recursive"
      title={`Рекурсивная ссылка на схему ${name} (вынесена в components/schemas)`}
    >
      ↻ {name}
    </span>
  );
}

export function SchemaViewer({ schema, name, depth = 0, required = false }: SchemaViewerProps) {
  const [collapsed, setCollapsed] = useState(depth >= 2);

  if (isRefMarker(schema)) {
    return (
      <div className="oas-schema-row" style={{ paddingLeft: depth * 14 }}>
        {name ? <span className="oas-schema-name">{name}</span> : null}
        <RefBadge marker={schema} />
      </div>
    );
  }

  const recursiveRef = localSchemaRefName(schema);
  if (recursiveRef) {
    return (
      <div className="oas-schema-row" style={{ paddingLeft: depth * 14 }}>
        {name ? (
          <span className="oas-schema-name">
            {name}
            {required ? <span className="oas-schema-required">*</span> : null}
          </span>
        ) : null}
        <RecursiveRefBadge name={recursiveRef} />
      </div>
    );
  }

  const s = asObject(schema);
  if (!s) {
    return (
      <div className="oas-schema-row" style={{ paddingLeft: depth * 14 }}>
        {name ? <span className="oas-schema-name">{name}</span> : null}
        <span className="oas-schema-empty">—</span>
      </div>
    );
  }

  const properties = asObject(s.properties);
  const requiredList = Array.isArray(s.required)
    ? s.required.filter((r): r is string => typeof r === "string")
    : [];
  const items = s.items;
  const allOf = asSchemaArray(s.allOf);
  const oneOf = asSchemaArray(s.oneOf);
  const anyOf = asSchemaArray(s.anyOf);
  const enumValues = asSchemaArray(s.enum);
  const description = typeof s.description === "string" ? s.description : null;
  const example = compatibleExampleForSchema(s);
  const hasChildren = Boolean(properties || items || allOf || oneOf || anyOf);

  return (
    <div className="oas-schema-node">
      <div className="oas-schema-row" style={{ paddingLeft: depth * 14 }}>
        {hasChildren ? (
          <button
            type="button"
            className="oas-schema-twist"
            onClick={() => setCollapsed((c) => !c)}
            aria-label={collapsed ? "Развернуть" : "Свернуть"}
          >
            {collapsed ? "▸" : "▾"}
          </button>
        ) : (
          <span className="oas-schema-twist oas-schema-twist-empty" />
        )}
        {name ? (
          <span className="oas-schema-name">
            {name}
            {required ? <span className="oas-schema-required">*</span> : null}
          </span>
        ) : null}
        <span className="oas-schema-type">{typeLabel(s)}</span>
        {enumValues ? (
          <span className="oas-schema-enum">
            [{enumValues.map((v) => JSON.stringify(v)).join(", ")}]
          </span>
        ) : null}
        {description ? <span className="oas-schema-desc">{description}</span> : null}
      </div>

      {!collapsed && example !== undefined ? (
        <div className="oas-schema-example" style={{ paddingLeft: depth * 14 + 18 }}>
          example: <code>{JSON.stringify(example)}</code>
        </div>
      ) : null}

      {!collapsed && properties ? (
        <div className="oas-schema-children">
          {Object.entries(properties).map(([key, value]) => (
            <SchemaViewer
              key={key}
              schema={value}
              name={key}
              depth={depth + 1}
              required={requiredList.includes(key)}
            />
          ))}
        </div>
      ) : null}

      {!collapsed && items ? (
        <div className="oas-schema-children">
          <SchemaViewer schema={items} name="[items]" depth={depth + 1} />
        </div>
      ) : null}

      {!collapsed && allOf ? (
        <div className="oas-schema-children">
          {allOf.map((sub, i) => (
            <SchemaViewer key={i} schema={sub} name={`allOf[${i}]`} depth={depth + 1} />
          ))}
        </div>
      ) : null}

      {!collapsed && oneOf ? (
        <div className="oas-schema-children">
          {oneOf.map((sub, i) => (
            <SchemaViewer key={i} schema={sub} name={`oneOf[${i}]`} depth={depth + 1} />
          ))}
        </div>
      ) : null}

      {!collapsed && anyOf ? (
        <div className="oas-schema-children">
          {anyOf.map((sub, i) => (
            <SchemaViewer key={i} schema={sub} name={`anyOf[${i}]`} depth={depth + 1} />
          ))}
        </div>
      ) : null}
    </div>
  );
}

/** One-line inline type summary, for use in compact contexts like the
 * parameters table's "type" column. */
export function SchemaTypeInline({ schema }: { schema: unknown }) {
  if (isRefMarker(schema)) {
    return <RefBadge marker={schema} />;
  }
  const recursiveRef = localSchemaRefName(schema);
  if (recursiveRef) {
    return <RecursiveRefBadge name={recursiveRef} />;
  }
  const s = asObject(schema);
  if (!s) return <span className="oas-schema-empty">—</span>;
  const enumValues = asSchemaArray(s.enum);
  return (
    <span className="oas-schema-type-inline">
      {typeLabel(s)}
      {enumValues ? ` [${enumValues.map((v) => JSON.stringify(v)).join(", ")}]` : ""}
    </span>
  );
}
