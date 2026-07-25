import { useCallback, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import "./PanelResizeHandle.css";

type PanelResizeHandleProps = {
  direction: "horizontal" | "vertical";
  /** Positive delta grows the panel this handle is attached to (see invert). */
  onResize: (delta: number) => void;
  onResizeEnd?: () => void;
  /** When true, positive pointer movement shrinks the panel (e.g. right dock). */
  invert?: boolean;
  disabled?: boolean;
  ariaLabel: string;
};

export function PanelResizeHandle({
  direction,
  onResize,
  onResizeEnd,
  invert = false,
  disabled = false,
  ariaLabel,
}: PanelResizeHandleProps) {
  const [active, setActive] = useState(false);
  const lastPos = useRef(0);

  const onPointerDown = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (disabled) return;
      event.preventDefault();
      const target = event.currentTarget;
      target.setPointerCapture(event.pointerId);
      lastPos.current =
        direction === "horizontal" ? event.clientX : event.clientY;
      setActive(true);
      document.body.style.userSelect = "none";
      document.body.style.cursor =
        direction === "horizontal" ? "col-resize" : "row-resize";
    },
    [direction, disabled],
  );

  const onPointerMove = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (!active) return;
      const pos = direction === "horizontal" ? event.clientX : event.clientY;
      const raw = pos - lastPos.current;
      lastPos.current = pos;
      if (raw === 0) return;
      onResize(invert ? -raw : raw);
    },
    [active, direction, invert, onResize],
  );

  const endDrag = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (!active) return;
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
      setActive(false);
      document.body.style.userSelect = "";
      document.body.style.cursor = "";
      onResizeEnd?.();
    },
    [active, onResizeEnd],
  );

  if (disabled) return null;

  return (
    <div
      className={`panel-resize-handle ${direction}${active ? " is-active" : ""}`}
      role="separator"
      aria-orientation={direction === "horizontal" ? "vertical" : "horizontal"}
      aria-label={ariaLabel}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
    />
  );
}
