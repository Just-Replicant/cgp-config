use cgp_fork::prelude::*;

use crate::{SecretGuard, SecretSessionComponent, SecretSessionOpener};

/// Refuses secret references. The preset provider: a `Secret<T>` literal is kept, and
/// a reference fails while it is deserialized.
pub struct NoSecretSession;

#[cgp_impl(NoSecretSession: SecretSessionComponent)]
impl SecretSessionOpener {
    fn open_secret_session(&self) -> SecretGuard {
        SecretGuard::deny()
    }
}
