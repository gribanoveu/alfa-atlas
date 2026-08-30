import { CATEGORY_PREFIX, DIMENSION_SLOT } from "../lib/metrics/constants";
import type { Metric } from "../lib/metrics/types";

/**
 * Every Alfa Atlas metric, in one place. Adding an event means adding it
 * here first — see METRICS.md for the naming rules and the dimension-slot
 * registry.
 */
export const EventCategory = {
  app: `${CATEGORY_PREFIX} > App`,
  assistant: `${CATEGORY_PREFIX} > Assistant`,
} as const;

/** Where a project open came from. Closed list — goes into `property`. */
export type ProjectOpenSource = "restore" | "recent" | "dialog" | "clone";

/**
 * Who caused a mode switch, and — when it was the assistant asking — what
 * the user answered. Closed list, goes into `label`.
 *
 * The distinction is the point: "the user chose full-repo access" and "the
 * assistant asked for it and the user agreed" are different facts about
 * trust, and a single "mode changed" count would conflate them.
 */
export type ModeSwitchOrigin = "user" | "assistant-granted" | "assistant-denied";

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
    /**
     * A project was opened. `label` says whether it is a git repository,
     * `property` how the user got there. Neither the path nor the
     * repository name is ever sent — see the privacy checklist in
     * METRICS.md.
     */
    OPEN_PROJECT: {
      category: EventCategory.app,
      action: "Open -> Project",
      label: "Project opened",
    } satisfies Metric,
  },

  ASSISTANT: {
    /**
     * Fires once for every turn that ends, whatever the outcome — so the
     * success rate is a count over `label` and needs no join. `value` is
     * the turn's duration in seconds, `property` the access mode.
     */
    SETTLE_TURN: {
      category: EventCategory.assistant,
      action: "Settle -> Turn",
      label: "Turn settled",
      dimensionsMapping: { providerId: DIMENSION_SLOT.providerId },
    } satisfies Metric,

    /**
     * The diagnosis behind a `Settle -> Turn` with `label: "error"`.
     * Deliberately a second event rather than another field: the funnel
     * stays one event per turn, and the breakdown lives on its own.
     * `label` is the error class, never the provider's error text.
     */
    FAIL_TURN: {
      category: EventCategory.assistant,
      action: "Fail -> Turn",
      label: "Turn failed",
      dimensionsMapping: { providerId: DIMENSION_SLOT.providerId },
    } satisfies Metric,

    /**
     * One event per approval round, not per call — a round can hold a
     * dozen. `label` is the round's verdict, `value` how many calls it
     * covered.
     */
    DECIDE_TOOLS: {
      category: EventCategory.assistant,
      action: "Decide -> Tool calls",
      label: "Tool calls decided",
      dimensionsMapping: { providerId: DIMENSION_SLOT.providerId },
    } satisfies Metric,

    /**
     * Aggregated per turn, per tool, per outcome: `property` is the tool
     * name, `label` whether it succeeded, `value` how many such calls the
     * turn made. A turn with thirty `readFile` calls costs one event, not
     * thirty — which is what keeps tool traffic from evicting rarer,
     * more valuable events from the delivery queue.
     */
    RUN_TOOL: {
      category: EventCategory.assistant,
      action: "Run -> Tool",
      label: "Tool run",
      dimensionsMapping: { providerId: DIMENSION_SLOT.providerId },
    } satisfies Metric,

    /**
     * History compaction fired — the signal that a conversation is
     * pressing against the context window. `label` says whether it ran on
     * its own or because the user retried a failed turn.
     */
    /**
     * Access mode changed — `docsOnly` ↔ `fullRepo`. `label` says who did
     * it and, when the assistant asked, what the user answered:
     * `user`, `assistant-granted`, `assistant-denied`. `property` is the
     * mode being moved to.
     */
    SWITCH_ACCESS_MODE: {
      category: EventCategory.assistant,
      action: "Switch -> Access mode",
      label: "Access mode switched",
      dimensionsMapping: { providerId: DIMENSION_SLOT.providerId },
    } satisfies Metric,

    /**
     * Conversation mode changed — `agent` / `plan` / `question`. Same
     * `label` vocabulary as the access mode: who initiated it, and how the
     * user answered when it was the assistant asking.
     */
    SWITCH_CONVERSATION_MODE: {
      category: EventCategory.assistant,
      action: "Switch -> Conversation mode",
      label: "Conversation mode switched",
      dimensionsMapping: { providerId: DIMENSION_SLOT.providerId },
    } satisfies Metric,

    COMPACT_CONTEXT: {
      category: EventCategory.assistant,
      action: "Compact -> Context",
      label: "Context compacted",
      dimensionsMapping: { providerId: DIMENSION_SLOT.providerId },
    } satisfies Metric,
  },
} as const;
