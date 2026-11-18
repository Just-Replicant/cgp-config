use cgp_fork::prelude::*;

use crate::{SecretsResolver, SecretsResolverComponent};

/// The preset secrets stage until a secrets preset is selected.
///
/// **In this release `DenySecrets` is a no-op: it always succeeds and rejects nothing.**
///
/// The value tree stores secret *references* and only `Secret<T>` fields hold resolved
/// bytes. Neither `Secret<T>` nor the generated metadata that lists a config's secret
/// fields exists yet, so this provider has no way to see a secret field and nothing to
/// resolve. Rejection (`ConfigError::SecretsDenied` for a config with unresolved secret
/// fields) arrives with the `secrets` feature; until then, do not rely on this provider to
/// keep secrets out of a config.
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
