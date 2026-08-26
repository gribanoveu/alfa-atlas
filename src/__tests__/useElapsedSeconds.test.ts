import { describe, expect, test } from "bun:test";
import { renderHook } from "@testing-library/react";
import { useElapsedSeconds } from "../hooks/useElapsedSeconds";

describe("useElapsedSeconds", () => {
  test("counts seconds since startedAt while running", () => {
    const startedAt = Date.now() - 5_000;
    const { result } = renderHook(() => useElapsedSeconds(startedAt, true));
    expect(result.current).toBeGreaterThanOrEqual(5);
    expect(result.current).toBeLessThan(7);
  });

  test("latches the final value once it stops running", () => {
    const startedAt = Date.now() - 3_000;
    const { result, rerender } = renderHook(
      ({ running }) => useElapsedSeconds(startedAt, running),
      { initialProps: { running: false } },
    );

    const frozen = result.current;
    // A later unrelated re-render must not drift the frozen duration.
    rerender({ running: false });
    expect(result.current).toBe(frozen);
  });
});
