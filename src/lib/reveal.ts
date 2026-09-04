import { invoke } from "@tauri-apps/api/core";

/** Shows `path` in the OS file manager, or opens it with its default app.
 *
 * Goes through the Rust side rather than the opener plugin's `openPath`:
 * the capability's path scope is static and cannot express "inside the
 * project that happens to be open right now", so granting `openPath` from
 * the webview meant granting every path on the machine. The containment
 * check lives in `commands::reveal` instead, where the roots can be read at
 * call time. Paths outside `~/.atlas` and the open project are rejected
 * there. */
export function revealPath(path: string): Promise<void> {
  return invoke<void>("reveal_path", { path });
}
