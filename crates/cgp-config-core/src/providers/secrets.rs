use cgp_fork::prelude::*;

use crate::{SecretsResolver, SecretsResolverComponent};

/// The preset secrets stage.
///
/// A `Secret<T>` reference is resolved or refused while the value is deserialized,
/// under the secret session `LoadLayered` opens first. With the preset session
/// (`NoSecretSession`) a reference fails with `ConfigError::SecretsDenied` and a
/// literal is kept. A resolving session (the `secrets` feature) fills the secret in
/// before this stage runs, so this provider does not walk the config. `LazySecret<T>`
/// stays unresolved until it is read.
pub struct DenySecrets;

#[cgp_impl(DenySecrets)]
impl<Config> SecretsResolver<Config>
where
    Self: HasErrorType,
{
    fn resolve_secrets(&self, _config: &mut Config) -> Result<(), Self::Error> {
        Ok(())
    }
}
