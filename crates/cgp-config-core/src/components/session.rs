use cgp_fork::prelude::*;

use crate::SecretGuard;

/// Opens the secret session for the deserialization that follows.
///
/// `LoadLayered` holds the returned guard until `Secret<T>` has been deserialized.
/// The preset provider refuses references. A resolving provider (feature `secrets`)
/// installs the env, file, and docker readers.
#[cgp_component {
    name: SecretSessionComponent,
    provider: SecretSessionOpener,
}]
pub trait CanOpenSecretSession {
    /// The guard that restores the previous session when dropped.
    fn open_secret_session(&self) -> SecretGuard;
}
