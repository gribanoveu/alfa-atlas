import { writeText } from "@tauri-apps/plugin-clipboard-manager";

export async function copyToClipboard(text: string): Promise<void> {
  await writeText(text);
}

/** Empty string when the clipboard holds no text (an image, or nothing).
 *
 * `readText` is pulled in on demand rather than at the top of the file: only
 * the Paste path needs it, and a static binding would make this module fail
 * to evaluate under the several tests that replace the clipboard plugin with
 * a write-only stub. */
export async function readClipboardText(): Promise<string> {
  const { readText } = await import("@tauri-apps/plugin-clipboard-manager");
  return (await readText()) ?? "";
}
