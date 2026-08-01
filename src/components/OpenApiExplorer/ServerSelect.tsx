import { useEffect, useRef, useState } from "react";
import "./OpenApiExplorer.css";

type ServerSelectProps = {
  servers: string[];
  value: string;
  onSelect: (url: string) => void;
};

/** Programmatic dropdown (trigger button + absolute option list, closes on
 * outside click / Escape) — same pattern as the docs-root candidate picker
 * in `ConfirmOpenProjectModal.tsx` (`.clone-select*`), not a native
 * `<select>`, so it can be styled and behave consistently with the rest of
 * the app. Only picks a value into the adjacent free-text URL input — it
 * doesn't hold the URL itself, since the input stays editable for custom
 * hosts not listed in `servers`. */
export function ServerSelect({ servers, value, onSelect }: ServerSelectProps) {
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

  if (servers.length === 0) return null;

  const activeUrl = servers.includes(value) ? value : null;

  return (
    <div className="oas-select" ref={rootRef}>
      <button
        type="button"
        className={`oas-select-trigger${open ? " is-open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="oas-select-value">
          {activeUrl ?? <span className="oas-select-placeholder">известные серверы…</span>}
        </span>
        <span className="oas-select-chevron" aria-hidden>
          ▾
        </span>
      </button>
      {open ? (
        <div className="oas-select-menu" role="listbox">
          {servers.map((url) => (
            <button
              key={url}
              type="button"
              role="option"
              aria-selected={url === activeUrl}
              className={`oas-select-option${url === activeUrl ? " is-active" : ""}`}
              onClick={() => {
                onSelect(url);
                setOpen(false);
              }}
            >
              {url}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
