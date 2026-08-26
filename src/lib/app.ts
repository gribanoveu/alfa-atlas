import { invoke } from "@tauri-apps/api/core";

/** Quits the app. Registered directly in `lib.rs` rather than under
 * `commands/` — it has no service behind it, it just calls `app.exit(0)` —
 * but it still gets a wrapper here so no component reaches for `invoke`
 * itself. */
export function exitApp(): Promise<void> {
  return invoke<void>("exit_app");
}
