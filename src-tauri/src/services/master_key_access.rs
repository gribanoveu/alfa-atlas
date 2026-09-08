use serde::Serialize;

use crate::infra::master_key;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterKeyAccessStatus {
    /// The master key is in the OS keychain but this process cannot read it
    /// right now — stored credentials stay sealed until access is granted.
    pub unreachable: bool,
}

pub fn status() -> MasterKeyAccessStatus {
    MasterKeyAccessStatus {
        unreachable: master_key::existing_key_is_unreachable(),
    }
}

/// Re-asks the OS for access and reports where that left things.
///
/// The outcome of a retry *is* the access status — a denial is the expected
/// answer, not a fault — so it is not an `Err`. Returning one would put the
/// raw keychain message on screen, and those are written in English by the
/// platform, in a UI that is not.
pub fn retry() -> MasterKeyAccessStatus {
    let _ = master_key::retry_access();
    status()
}
