/**
 * Dimension-slot registry. Vendored conventions from `alfa-metrics-kit`;
 * the occupied slots below are ours.
 *
 * Keep this in step with the table in METRICS.md and with
 * `src-tauri/src/domain/metrics.rs`: a slot silently reused for a second
 * meaning makes every historical query over it wrong.
 */

/** Reserved for organizationId in corp web apps. Never used here. */
export const ORGANIZATION_DIMENSION_SLOT = "1";

export const DIMENSION_SLOT = {
  installId: "2",
  appVersion: "3",
  /**
   * Operating system family — "macos" / "windows" / "linux". Named `os`,
   * not `platform`: the tracker protocol's own `p` field already means
   * "platform" and carries "web" for every event.
   */
  os: "4",
  /**
   * Set in Rust, not here — a session id is meaningless outside the
   * running process. Listed so the registry stays complete.
   */
  sessionId: "5",
  /**
   * Which LLM provider a turn ran against. Only ever a *system* provider
   * id from the bundled manifest: a user-configured provider is reported
   * as "custom", enforced in Rust so no call site can leak the name
   * someone typed (it has been seen to contain internal hostnames).
   */
  providerId: "6",
} as const;

/** Category prefix, so every Alfa Atlas event is greppable in the warehouse. */
export const CATEGORY_PREFIX = "ALFA-ATLAS";
