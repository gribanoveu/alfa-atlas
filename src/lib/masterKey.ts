import { invoke } from "@tauri-apps/api/core";

export type MasterKeyAccessStatus = {
  unreachable: boolean;
};

/** Fired once the master key becomes readable again, so long-lived panels
 * that cached "no credential configured" can re-check.
 *
 * They load that answer when they mount and have no other way to learn it
 * changed: the assistant gates its composer on a cached per-provider flag,
 * and a window that started without keychain access would otherwise stay
 * dead until it was restarted. */
export const MASTER_KEY_UNLOCKED_EVENT = "atlas-master-key-unlocked";

export function getMasterKeyAccessStatus(): Promise<MasterKeyAccessStatus> {
  return invoke<MasterKeyAccessStatus>("master_key_get_access_status");
}

/** Re-asks the OS for access; resolves with the status that leaves behind,
 * denial included. */
export function retryMasterKeyAccess(): Promise<MasterKeyAccessStatus> {
  return invoke<MasterKeyAccessStatus>("master_key_retry_access");
}
