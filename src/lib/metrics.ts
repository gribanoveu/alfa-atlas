import { invoke } from "@tauri-apps/api/core";

import { mapDimensions } from "./metrics/mapDimensions";
import type { AdditionalMetricData, Metric } from "./metrics/types";

export type MetricsStatus = {
  enabled: boolean;
  installId: string | null;
  installReportedAt: number | null;
};

export function getMetricsStatus(): Promise<MetricsStatus> {
  return invoke("metrics_status");
}

export function setMetricsEnabled(enabled: boolean): Promise<MetricsStatus> {
  return invoke("metrics_set_enabled", { enabled });
}

/**
 * Sends one product-metrics event. Resolves the catalog's
 * `dimensionsMapping` into numbered slots here, so Rust never has to know
 * event-specific key names.
 *
 * Never rejects: metrics are best-effort and must not surface as an error
 * in a flow the user actually asked for. The same contract
 * `infra::tool_call_log` holds on the Rust side.
 */
export async function trackMetric(
  metric: Metric,
  additionalData?: AdditionalMetricData,
): Promise<void> {
  try {
    await invoke("metrics_track", {
      event: {
        category: metric.category,
        action: metric.action,
        label: metric.label,
        property: metric.property ?? null,
        value: metric.value ?? null,
        dimensions: mapDimensions(metric.dimensionsMapping, additionalData),
      },
    });
  } catch {
    // Intentionally swallowed — see the doc comment above.
  }
}
