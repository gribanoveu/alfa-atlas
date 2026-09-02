/** Three bars rising and falling in a staggered wave — the "something is
 * running" indicator for tool calls and every other in-flight card.
 *
 * Replaces a rotating `Loader2`. A rotating arc has to be redrawn on a new
 * sub-pixel offset every frame at this size, which is what made it read as
 * wobbling around its axis; three bars only ever scale along one axis from a
 * fixed baseline, so there is no rotation to land off-centre.
 *
 * `aria-hidden` because the surrounding card already says what is happening
 * in words — the bars are decoration on top of that label, not the only
 * announcement of it. */
export function AssistantLoadingBars({ className }: { className?: string }) {
  return (
    <span className={`assistant-loading-bars${className ? ` ${className}` : ""}`} aria-hidden>
      <span />
      <span />
      <span />
    </span>
  );
}
