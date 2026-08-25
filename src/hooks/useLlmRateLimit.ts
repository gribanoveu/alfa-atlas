import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getLlmRateLimitSnapshot, type RateLimitSnapshot } from "../lib/llm";

/**
 * Polls the backend rate-limit snapshot for the status-bar EVC chip.
 *
 * Refresh cadence:
 * - immediate on mount / provider change / `llm:rate-limit-changed` event
 * - every 1s while the popover is open or the provider is limited (live countdown)
 * - every 5s otherwise (events age out of the sliding window on their own)
 *
 * Returns `null` while loading, when there is no provider, or when the
 * policy is `"none"` (chip should stay hidden).
 */
export function useLlmRateLimit(providerId: string | null) {
  const [snapshot, setSnapshot] = useState<RateLimitSnapshot | null>(null);
  const [popoverOpen, setPopoverOpen] = useState(false);
  const providerIdRef = useRef(providerId);
  providerIdRef.current = providerId;

  const refresh = useCallback(async () => {
    const id = providerIdRef.current;
    if (!id) {
      setSnapshot(null);
      return;
    }
    try {
      const next = await getLlmRateLimitSnapshot(id);
      if (providerIdRef.current !== id) return;
      setSnapshot(next.policyId === "none" ? null : next);
    } catch {
      if (providerIdRef.current !== id) return;
      setSnapshot(null);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [providerId, refresh]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    void listen("llm:rate-limit-changed", () => {
      void refresh();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [refresh]);

  useEffect(() => {
    if (!providerId) return;
    // Keep a slow poll even while the chip is hidden (`snapshot === null`)
    // so turning tracking back on in Settings is picked up without a
    // provider switch. Fast poll only while the popover is open or the
    // provider is currently limited.
    const fast = popoverOpen || Boolean(snapshot?.isLimited);
    const ms = fast ? 1000 : 5000;
    const timer = window.setInterval(() => {
      void refresh();
    }, ms);
    return () => window.clearInterval(timer);
  }, [providerId, snapshot?.isLimited, popoverOpen, refresh]);

  return {
    snapshot,
    refresh,
    popoverOpen,
    setPopoverOpen,
  };
}
