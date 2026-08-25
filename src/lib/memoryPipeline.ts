import { invoke } from "@tauri-apps/api/core";

/** Fire-and-forget: queues a background extractor job for the chat's new
 * turns. Resolves as soon as the job is spawned — memory write never
 * blocks the assistant reply. Failures are logged on the Rust side. */
export function memoryExtractTurn(repoRoot: string, chatId: string): Promise<void> {
  return invoke("memory_extract_turn", { repoRoot, chatId });
}
