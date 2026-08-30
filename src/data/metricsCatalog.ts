import { CATEGORY_PREFIX, DIMENSION_SLOT } from "../lib/metrics/constants";
import type { Metric } from "../lib/metrics/types";

/**
 * Every Alfa Atlas metric, in one place. Adding an event means adding it
 * here first — see METRICS.md for the naming rules and the dimension-slot
 * registry.
 */
export const EventCategory = {
  app: `${CATEGORY_PREFIX} > App`,
} as const;

export const METRICS = {
  APP: {
    /**
     * Reported at most once per `~/.atlas` profile. Built and sent from
     * Rust (`services::metrics::report_install_once`) so it can fire before
     * the webview exists and retry across launches; this entry is the
     * source of truth for its shape.
     */
    INSTALL: {
      category: EventCategory.app,
      action: 'Install -> First launch',
      label: "App installed",
      dimensionsMapping: {
        installId: DIMENSION_SLOT.installId,
        appVersion: DIMENSION_SLOT.appVersion,
        os: DIMENSION_SLOT.os,
      },
    } satisfies Metric,
  },
} as const;
