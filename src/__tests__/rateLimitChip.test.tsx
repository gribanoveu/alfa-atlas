import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { RateLimitChip } from "../components/StatusBar/RateLimitChip";
import type {
  RateLimitResource,
  RateLimitResourceKind,
  RateLimitSnapshot,
} from "../lib/llm";

afterEach(cleanup);

const NOW = Date.UTC(2026, 6, 15, 9, 0, 0);
const WINDOW_MS = 20 * 60_000;

function resource(
  kind: RateLimitResourceKind,
  used: number,
  limit: number,
  overrides: Partial<RateLimitResource> = {},
): RateLimitResource {
  return {
    kind,
    used,
    limit,
    remaining: Math.max(0, limit - used),
    isLimited: used >= limit,
    severity: used >= limit ? "limited" : "normal",
    retryUntil: null,
    nextReleaseAt: NOW + 60_000,
    nextReleaseAmount: Math.min(used, 500),
    ...overrides,
  };
}

function snapshot(overrides: Partial<RateLimitSnapshot> = {}): RateLimitSnapshot {
  const resources = overrides.resources ?? [
    resource("prompt", 2_500_000, 10_000_000),
    resource("completion", 12_000, 60_000),
    resource("requests", 40, 1_000),
  ];
  return {
    policyId: "evc-sliding-window",
    label: "EVC",
    used: 12_000,
    remaining: 48_000,
    limit: 60_000,
    drivingKind: "completion",
    windowMs: WINDOW_MS,
    isEnforced: true,
    isLimited: false,
    severity: "normal",
    retryUntil: null,
    nextReleaseAt: NOW + 60_000,
    nextEnforceAt: null,
    offHoursOverride: false,
    releases: [],
    samples: [],
    ...overrides,
    resources,
  };
}

function openPopover(snap: RateLimitSnapshot) {
  const view = render(<RateLimitChip snapshot={snap} open onOpenChange={() => {}} />);
  return view;
}

describe("RateLimitChip", () => {
  test("the chip shows the driving counter, not a fixed one", () => {
    // Prompt is at 95% while completion idles: the one number on the chip
    // has to be the counter that will actually refuse the next request.
    const snap = snapshot({
      resources: [
        resource("prompt", 9_500_000, 10_000_000, { severity: "critical" }),
        resource("completion", 1_000, 60_000),
        resource("requests", 10, 1_000),
      ],
      drivingKind: "prompt",
      used: 9_500_000,
      remaining: 500_000,
      limit: 10_000_000,
      severity: "critical",
    });
    render(<RateLimitChip snapshot={snap} open={false} onOpenChange={() => {}} />);
    expect(screen.getByText("9.5M / 10M")).toBeDefined();
  });

  test("the popover breaks the window down into all three counters", () => {
    openPopover(snapshot());
    expect(screen.getByText("Запрос")).toBeDefined();
    expect(screen.getByText("Ответ")).toBeDefined();
    expect(screen.getByText("Обращения")).toBeDefined();
    // Window length comes from the snapshot, never hard-coded in the UI.
    expect(screen.getByText(/окно 20 мин/)).toBeDefined();
  });

  test("one limited counter is explained once, by the shared hint", () => {
    const snap = snapshot({
      resources: [
        resource("prompt", 100, 10_000_000),
        resource("completion", 65_000, 60_000, {
          retryUntil: NOW + 120_000,
          severity: "limited",
        }),
        resource("requests", 40, 1_000),
      ],
      drivingKind: "completion",
      used: 65_000,
      remaining: 0,
      isLimited: true,
      severity: "limited",
      retryUntil: NOW + 120_000,
    });
    openPopover(snap);
    expect(screen.getByText(/Повтор с/)).toBeDefined();
    // The row note would just repeat the hint's time.
    expect(screen.queryByText(/освободится/)).toBeNull();
  });

  test("several limited counters get their own times", () => {
    // The shared hint can only name the latest of them, so each row says
    // when it personally clears.
    const snap = snapshot({
      resources: [
        resource("prompt", 100, 10_000_000),
        resource("completion", 65_000, 60_000, {
          retryUntil: NOW + 600_000,
          severity: "limited",
        }),
        resource("requests", 1_000, 1_000, {
          retryUntil: NOW + 120_000,
          severity: "limited",
        }),
      ],
      drivingKind: "completion",
      used: 65_000,
      remaining: 0,
      isLimited: true,
      severity: "limited",
      retryUntil: NOW + 600_000,
    });
    openPopover(snap);
    expect(screen.getAllByText(/освободится/).length).toBe(2);
  });

  test("off hours read as unlimited and point at the next enforced moment", () => {
    const snap = snapshot({
      isEnforced: false,
      severity: "offHours",
      nextEnforceAt: NOW + 3 * 3_600_000,
    });
    render(<RateLimitChip snapshot={snap} open onOpenChange={() => {}} />);
    expect(screen.getByText("без лимита")).toBeDefined();
    expect(screen.getByText(/Лимит с /)).toBeDefined();
  });

  test("the override explains why the chip counts outside working hours", () => {
    openPopover(snapshot({ offHoursOverride: true }));
    expect(screen.getByText(/подсчёт включён в настройках/)).toBeDefined();
  });

  test("clicking the chip toggles the popover", () => {
    let opened: boolean | null = null;
    render(
      <RateLimitChip snapshot={snapshot()} open={false} onOpenChange={(v) => (opened = v)} />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(opened).toBe(true);
  });
});
