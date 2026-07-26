import {
  countLabel,
  firstEntryHint,
  isEmptyValue,
  valueKind,
} from "./structuredDataUtils";
import type { StructuredValue } from "./structuredDataUtils";

type TreeNodeProps = {
  data: StructuredValue;
  label?: string | number;
  path: string;
  expanded: ReadonlySet<string>;
  onToggle: (path: string) => void;
};

function ValueLeaf({ value }: { value: StructuredValue }) {
  if (isEmptyValue(value)) {
    return <span className="struct-punct">null</span>;
  }

  const kind = valueKind(value);
  if (kind === "number") {
    return <span className="struct-number">{String(value)}</span>;
  }
  if (kind === "bool") {
    return <span className="struct-bool">{String(value)}</span>;
  }
  return <span className="struct-string">&quot;{String(value)}&quot;</span>;
}

function TreeLabel({ label }: { label: string | number }) {
  return (
    <>
      <span className="struct-key">{String(label)}</span>
      <span className="struct-punct">: </span>
    </>
  );
}

export function TreeNode({
  data,
  label,
  path,
  expanded,
  onToggle,
}: TreeNodeProps) {
  const isObject = data !== null && typeof data === "object";
  const isArray = Array.isArray(data);

  if (!isObject) {
    return (
      <div className="struct-row struct-leaf">
        {label !== undefined ? <TreeLabel label={label} /> : null}
        <ValueLeaf value={data} />
      </div>
    );
  }

  const entries = isArray
    ? data.map((value, index) => [index, value] as const)
    : Object.entries(data);
  const count = entries.length;
  const firstHint = firstEntryHint(entries, isArray);
  const isOpen = expanded.has(path);
  const openBracket = isArray ? "[" : "{";
  const closeBracket = isArray ? "]" : "}";

  return (
    <div className="struct-branch">
      <button
        type="button"
        className="struct-row struct-row-clickable"
        onClick={() => onToggle(path)}
        aria-expanded={isOpen}
      >
        <span className="struct-twist">{isOpen ? "▾" : "▸"}</span>
        {label !== undefined ? <TreeLabel label={label} /> : null}
        <span className="struct-punct">{openBracket}</span>
        {!isOpen ? (
          <>
            <span className="struct-count">{countLabel(count, isArray)}</span>
            {firstHint ? (
              <span className="struct-hint-wrap">
                <span className="struct-hint-sep">·</span>
                <span className="struct-hint-entry">
                  {firstHint.key ? (
                    <>
                      <span className="struct-hint struct-hint-key">
                        {firstHint.key}
                      </span>
                      <span className="struct-hint struct-hint-punct">: </span>
                    </>
                  ) : null}
                  <span
                    className={`struct-hint struct-hint-${firstHint.valueKind}`}
                  >
                    {firstHint.valuePreview}
                  </span>
                </span>
              </span>
            ) : null}
            <span className="struct-punct struct-close-inline">{closeBracket}</span>
          </>
        ) : null}
        {isOpen && count === 0 ? (
          <span className="struct-punct">{closeBracket}</span>
        ) : null}
      </button>

      {isOpen && count > 0 ? (
        <>
          <div className="struct-children">
            {entries.map(([key, value]) => (
              <TreeNode
                key={String(key)}
                data={value}
                label={isArray ? undefined : key}
                path={`${path}/${key}`}
                expanded={expanded}
                onToggle={onToggle}
              />
            ))}
          </div>
          <div className="struct-row struct-close">
            <span className="struct-punct">{closeBracket}</span>
          </div>
        </>
      ) : null}
    </div>
  );
}
