import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
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

function clearBodyDragStyles() {
  document.body.style.userSelect = "";
  document.body.style.cursor = "";
}

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
  const activeRef = useRef(false);
  const onResizeRef = useRef(onResize);
  const onResizeEndRef = useRef(onResizeEnd);
  const invertRef = useRef(invert);
  const directionRef = useRef(direction);

  onResizeRef.current = onResize;
  onResizeEndRef.current = onResizeEnd;
  invertRef.current = invert;
  directionRef.current = direction;
  activeRef.current = active;

  const finishDrag = useCallback(() => {
    if (!activeRef.current) return;
    activeRef.current = false;
    setActive(false);
    clearBodyDragStyles();
    onResizeEndRef.current?.();
  }, []);

  useEffect(() => {
    if (!active) return;

    const onPointerMove = (event: PointerEvent) => {
      if (!activeRef.current) return;
      const pos =
        directionRef.current === "horizontal" ? event.clientX : event.clientY;
      const raw = pos - lastPos.current;
      lastPos.current = pos;
      if (raw === 0) return;
      onResizeRef.current(invertRef.current ? -raw : raw);
    };

    const onPointerUp = () => {
      finishDrag();
    };

    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerUp);

    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", onPointerUp);
      // Panel may unmount mid-drag (drag-to-collapse) — clear cursor/styles.
      if (activeRef.current) {
        finishDrag();
      } else {
        clearBodyDragStyles();
      }
    };
  }, [active, finishDrag]);

  const onPointerDown = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (disabled) return;
      event.preventDefault();
      lastPos.current =
        direction === "horizontal" ? event.clientX : event.clientY;
      activeRef.current = true;
      setActive(true);
      document.body.style.userSelect = "none";
      document.body.style.cursor =
        direction === "horizontal" ? "col-resize" : "row-resize";
    },
    [direction, disabled],
  );

  if (disabled) return null;

  return (
    <div
      className={`panel-resize-handle ${direction}${active ? " is-active" : ""}`}
      role="separator"
      aria-orientation={direction === "horizontal" ? "vertical" : "horizontal"}
      aria-label={ariaLabel}
      onPointerDown={onPointerDown}
    />
  );
}
