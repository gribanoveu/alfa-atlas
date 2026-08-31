import { describe, expect, test } from "bun:test";

import { formatGitBusyLabel } from "../hooks/useGitProgress";

describe("formatGitBusyLabel", () => {
  test("a phase replaces the verb instead of being appended to it", () => {
    // The reported symptom: the button read «Клонирование… Аутентификация…» —
    // two operations at once, and two ellipses.
    expect(
      formatGitBusyLabel("Клонирование", {
        kind: "phase",
        op: "clone",
        phase: "authenticating",
        detail: "stored key",
      }),
    ).toBe("Аутентификация…");
  });

  test("checkout counts speak for themselves too", () => {
    expect(
      formatGitBusyLabel("Клонирование", {
        kind: "checkout",
        op: "clone",
        completed: 3,
        total: 10,
        path: null,
      }),
    ).toBe("Распаковка 3/10");
  });

  test("a bare percentage keeps the verb — it has no words of its own", () => {
    expect(
      formatGitBusyLabel("Клонирование", {
        kind: "transfer",
        op: "clone",
        receivedObjects: 21,
        totalObjects: 50,
        receivedBytes: 1024,
        indexedDeltas: 0,
        totalDeltas: 0,
      }),
    ).toBe("Клонирование… 42%");
  });

  test("without an event the verb stands alone", () => {
    expect(formatGitBusyLabel("Отправка", null)).toBe("Отправка…");
  });

  test("a transfer that knows nothing yet falls back to the verb", () => {
    expect(
      formatGitBusyLabel("Обновление", {
        kind: "transfer",
        op: "pull",
        receivedObjects: 0,
        totalObjects: 0,
        receivedBytes: 0,
        indexedDeltas: 0,
        totalDeltas: 0,
      }),
    ).toBe("Обновление…");
  });
});
