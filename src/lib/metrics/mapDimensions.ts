import type { AdditionalMetricData, Metric } from "./types";

/**
 * Converts { reportId: 'abc' } + { reportId: '2' } → { '2': 'abc' } for the
 * Snowplow custom_dimension schema. Vendored from `alfa-metrics-kit`, with
 * values coerced to strings: the tracker protocol is string-typed, and a
 * numeric slot value is rejected downstream.
 */
export function mapDimensions(
  dimensionMap: Metric["dimensionsMapping"],
  dimensionData?: AdditionalMetricData,
): Record<string, string> {
  if (!dimensionData || !dimensionMap) {
    return {};
  }

  const result: Record<string, string> = {};

  for (const [key, slot] of Object.entries(dimensionMap)) {
    if (!slot || !(key in dimensionData)) {
      continue;
    }

    const value = dimensionData[key];

    if (value !== undefined) {
      result[slot] = String(value);
    }
  }

  return result;
}
