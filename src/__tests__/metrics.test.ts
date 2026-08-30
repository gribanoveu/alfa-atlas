import { beforeEach, describe, expect, mock, test } from "bun:test";

import { mapDimensions } from "../lib/metrics/mapDimensions";
import { DIMENSION_SLOT, ORGANIZATION_DIMENSION_SLOT } from "../lib/metrics/constants";
import { METRICS } from "../data/metricsCatalog";

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

describe("metrics catalog", () => {
  test("no event claims the slot reserved for organizationId", () => {
    for (const event of Object.values(METRICS.APP)) {
      const slots = Object.values(event.dimensionsMapping ?? {});
      expect(slots).not.toContain(ORGANIZATION_DIMENSION_SLOT);
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

  test("never rejects, so a metrics failure cannot break the caller's flow", async () => {
    invokeThrows = "collector unreachable";
    await expect(trackMetric(METRICS.APP.INSTALL)).resolves.toBeUndefined();
  });
});
