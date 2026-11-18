use cgp_fork::prelude::*;

use crate::{SecretsResolver, SecretsResolverComponent};

/// The preset secrets stage until a secrets preset is selected.
///
/// The value tree stores secret *references* and only `Secret<T>` fields hold resolved
/// bytes. Until the `secrets` feature provides `Secret<T>` and the metadata that lists
/// secret fields, a config cannot contain an unresolved reference, so this provider has
/// nothing to resolve and succeeds. With secret fields present (`secrets` feature), it
/// fails the load with `ConfigError::SecretsDenied` instead of resolving them.
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
