/**
 * Event contract, vendored from `alfa-metrics-kit` in
 * `corp-wlbuh-ausn-ndfl-ui` (itself a port of `@alfa-bank/common-app-html`).
 *
 * Only the *shape* of an event is reused. The kit's transport — loading a
 * remote `sp.js` into the page — is not: see METRICS.md. Events are sent
 * through `src/lib/metrics.ts` → Rust instead.
 */

export type Metric = {
  category: string;
  action: string;
  label: string;
  property?: string | null;
  value?: number | null;
  /**
   * Maps additionalData keys to Snowplow dimension slots ('2'–'20').
   * Slot '1' is reserved for organizationId across Alfa Metrics — a desktop
   * app has no organization, so it stays empty here and is never reused.
   */
  dimensionsMapping?: Record<string, string>;
};

export type AdditionalMetricData = Record<
  string,
  string | number | boolean | undefined
>;
