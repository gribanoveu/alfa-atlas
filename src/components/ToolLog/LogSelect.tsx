import { useEffect, useRef, useState } from "react";
import "../Welcome/CloneRepoModal.css";
import "./ToolCallLogModal.css";

export type LogSelectOption = { value: string; label: string };

/** The filter dropdown of the log-style dialogs — the tool-call log and the
 *  artifacts list.
 *
 *  Same `.clone-select*` trigger/menu markup every other dropdown in the app
 *  hand-rolls per usage (`SettingsDialog`'s language picker,
 *  `AssistantConversation`'s model picker, …), compacted for a filter bar by
 *  `.tool-log-select`. It lived inside `ToolCallLogModal` while that was the
 *  only file with several of them side by side; it moved here when the
 *  artifacts dialog became the second, and it brings both stylesheets with
 *  it so a consumer cannot get the markup without the styles.
 *
 *  Not a native `<select>`: see the Style section of AGENTS.md. */
export function LogSelect({
  label,
  value,
  options,
  onChange,
  className,
}: {
  label: string;
  value: string;
  options: LogSelectOption[];
  onChange: (value: string) => void;
  /** Extra class on the wrapper, for a filter whose values are longer than
   *  the default 170px column — repository names, say. */
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
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

  const current = options.find((o) => o.value === value) ?? options[0];

  return (
    <div className={`clone-select tool-log-select${className ? ` ${className}` : ""}`} ref={ref}>
      <button
        type="button"
        className={`clone-select-trigger${open ? " is-open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={label}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="clone-select-value">
          <span className="clone-select-path">{current?.label}</span>
        </span>
        <span className="clone-select-chevron" aria-hidden>
          ▾
        </span>
      </button>
      {open ? (
        <div className="clone-select-menu" role="listbox">
          {options.map((option) => {
            const active = option.value === value;
            return (
              <button
                key={option.value}
                type="button"
                role="option"
                aria-selected={active}
                className={`clone-select-option${active ? " is-active" : ""}`}
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                }}
              >
                <span className="clone-select-path">{option.label}</span>
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}
