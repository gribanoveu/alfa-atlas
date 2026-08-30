import { beforeEach, describe, expect, mock, test } from "bun:test";

import { mapDimensions } from "../lib/metrics/mapDimensions";
import { DIMENSION_SLOT, ORGANIZATION_DIMENSION_SLOT } from "../lib/metrics/constants";
import { METRICS } from "../data/metricsCatalog";
import { classifyLlmError } from "../lib/metrics/classifyLlmError";

let invoked: Array<{ cmd: string; args: unknown }> = [];
let invokeThrows: string | null = null;

// `mock.module` is process-wide; this test file is the only one that
// touches the metrics wrapper, so a blanket stub of `invoke` is safe here.
mock.module("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string, args: unknown) => {
    invoked.push({ cmd, args });
    if (invokeThrows) throw invokeThrows;
    return undefined;
  },
}));

const { trackMetric } = await import("../lib/metrics");

beforeEach(() => {
  invoked = [];
  invokeThrows = null;
});

describe("mapDimensions", () => {
  test("resolves catalog keys into numbered slots", () => {
    expect(
      mapDimensions({ reportId: "2" }, { reportId: "abc" }),
    ).toEqual({ "2": "abc" });
  });

  test("coerces every value to a string, because the protocol is string-typed", () => {
    expect(
      mapDimensions(
        { count: "2", flag: "3" },
        { count: 42, flag: true },
      ),
    ).toEqual({ "2": "42", "3": "true" });
  });

  test("skips undefined values and unmapped keys", () => {
    expect(
      mapDimensions(
        { present: "2", absent: "3" },
        { present: "yes", absent: undefined, extra: "ignored" },
      ),
    ).toEqual({ "2": "yes" });
  });

  test("returns nothing when either side is missing", () => {
    expect(mapDimensions(undefined, { a: "b" })).toEqual({});
    expect(mapDimensions({ a: "2" }, undefined)).toEqual({});
  });
});

const ALL_EVENTS = [...Object.values(METRICS.APP), ...Object.values(METRICS.ASSISTANT)];

describe("metrics catalog", () => {
  test("no event claims the slot reserved for organizationId", () => {
    for (const event of ALL_EVENTS) {
      const slots = Object.values(event.dimensionsMapping ?? {});
      expect(slots).not.toContain(ORGANIZATION_DIMENSION_SLOT);
    }
  });

  test("every event maps only to slots in the registry", () => {
    const registered = new Set<string>(Object.values(DIMENSION_SLOT));
    for (const event of ALL_EVENTS) {
      for (const slot of Object.values(event.dimensionsMapping ?? {})) {
        expect(registered).toContain(slot);
      }
    }
  });

  test("every assistant event is sliceable by provider", () => {
    for (const event of Object.values(METRICS.ASSISTANT)) {
      expect(event.dimensionsMapping?.providerId).toBe(DIMENSION_SLOT.providerId);
    }
  });

  test("the install event maps installId and appVersion to their registered slots", () => {
    expect(METRICS.APP.INSTALL.dimensionsMapping).toEqual({
      installId: DIMENSION_SLOT.installId,
      appVersion: DIMENSION_SLOT.appVersion,
      os: DIMENSION_SLOT.os,
    });
  });

  test("every registered slot is distinct", () => {
    const slots = Object.values(DIMENSION_SLOT);
    expect(new Set(slots).size).toBe(slots.length);
  });
});

describe("classifyLlmError", () => {
  test.each([
    ["Rate limit exceeded, retry in 30s", "rateLimit"],
    ["HTTP 429 Too Many Requests", "rateLimit"],
    ["This model's maximum context length is 8192 tokens", "contextLength"],
    ["http status 401: invalid api key", "auth"],
    ["Connection timed out after 30000ms", "network"],
    ["tls handshake failed: certificate expired", "network"],
    ["http status 500: internal server error", "provider"],
    ["something nobody has seen before", "unknown"],
  ])("%s → %s", (message, expected) => {
    expect(classifyLlmError(message)).toBe(expected);
  });

  test("a rate limit is not mistaken for a generic provider error", () => {
    expect(classifyLlmError("http status 429: rate limit")).toBe("rateLimit");
  });

  test("returns a member of the closed set for any input", () => {
    const allowed = [
      "rateLimit", "contextLength", "auth", "network", "cancelled", "provider", "unknown",
    ];
    for (const message of ["", "/Users/eugene/secret.adoc not found", "стоп"]) {
      expect(allowed).toContain(classifyLlmError(message));
    }
  });

  test("never returns any part of the original message", () => {
    const leaky =
      "failed calling https://llm.internal.corp/v1 for user eugene: /Users/eugene/doc.adoc";
    expect(leaky).not.toContain(classifyLlmError(leaky));
  });
});

describe("trackMetric", () => {
  test("sends the resolved slots, not the catalog key names", async () => {
    await trackMetric(METRICS.APP.INSTALL, {
      installId: "install-uuid",
      appVersion: "0.3.1",
      os: "macos",
    });

    expect(invoked).toHaveLength(1);
    expect(invoked[0].cmd).toBe("metrics_track");
    expect(invoked[0].args).toEqual({
      event: {
        category: "ALFA-ATLAS > App",
        action: "Install -> First launch",
        label: "App installed",
        property: null,
        value: null,
        dimensions: { "2": "install-uuid", "3": "0.3.1", "4": "macos" },
      },
    });
  });

  test("overrides fill the per-occurrence parts without touching slots", async () => {
    await trackMetric(METRICS.APP.OPEN_PROJECT, undefined, {
      label: "git",
      property: "recent",
    });

    expect(invoked[0].args).toEqual({
      event: {
        category: "ALFA-ATLAS > App",
        action: "Open -> Project",
        label: "git",
        property: "recent",
        value: null,
        dimensions: {},
      },
    });
  });

  test("a project open carries no path, name or other identifying string", async () => {
    await trackMetric(METRICS.APP.OPEN_PROJECT, undefined, {
      label: "plain",
      property: "dialog",
    });

    const sent = JSON.stringify(invoked[0].args);
    for (const forbidden of ["/Users", "/home", "C:\\", ".git", "@", "http"]) {
      expect(sent).not.toContain(forbidden);
    }
  });

  test("a failed turn sends the error class, never the provider's text", async () => {
    const raw = "http status 500 at https://llm.internal.corp/v1 for /Users/eugene/doc.adoc";
    await trackMetric(
      METRICS.ASSISTANT.FAIL_TURN,
      { providerId: "alfagen" },
      { label: classifyLlmError(raw) },
    );

    const sent = JSON.stringify(invoked[0].args);
    expect(sent).toContain("provider");
    for (const forbidden of ["/Users", "internal.corp", "http", "doc.adoc"]) {
      expect(sent).not.toContain(forbidden);
    }
  });

  test("tool usage is reported per tool with a count, not per call", async () => {
    await trackMetric(
      METRICS.ASSISTANT.RUN_TOOL,
      { providerId: "alfagen" },
      { label: "ok", property: "readFile", value: 30 },
    );

    expect(invoked).toHaveLength(1);
    expect(invoked[0].args).toEqual({
      event: {
        category: "ALFA-ATLAS > Assistant",
        action: "Run -> Tool",
        label: "ok",
        property: "readFile",
        value: 30,
        dimensions: { "6": "alfagen" },
      },
    });
  });

  test.each([
    ["user", "fullRepo"],
    ["assistant-granted", "fullRepo"],
    ["assistant-denied", "fullRepo"],
  ])("an access-mode switch records %s", async (origin, mode) => {
    await trackMetric(METRICS.ASSISTANT.SWITCH_ACCESS_MODE, undefined, {
      label: origin,
      property: mode,
    });

    expect(invoked[0].args).toEqual({
      event: {
        category: "ALFA-ATLAS > Assistant",
        action: "Switch -> Access mode",
        label: origin,
        property: mode,
        value: null,
        dimensions: {},
      },
    });
  });

  test("a denied request is distinguishable from one never made", async () => {
    await trackMetric(METRICS.ASSISTANT.SWITCH_CONVERSATION_MODE, undefined, {
      label: "assistant-denied",
      property: "agent",
    });
    const event = (invoked[0].args as { event: { label: string; property: string } }).event;
    expect(event.label).toBe("assistant-denied");
    expect(event.property).toBe("agent");
  });

  test("never rejects, so a metrics failure cannot break the caller's flow", async () => {
    invokeThrows = "collector unreachable";
    await expect(trackMetric(METRICS.APP.INSTALL)).resolves.toBeUndefined();
  });
});
