use cgp_fork::prelude::*;

/// Resolves every secret reference of a deserialized config in place.
///
/// The value tree only holds references; this stage reads the secret bytes into the
/// config's `Secret<T>` fields. `DenySecrets` is the preset provider until the secrets
/// preset is selected. Per-scheme resolvers (`env:`, `file:`, `docker:`) are a separate,
/// per-scheme component that arrives with the `secrets` feature.
#[cgp_component(SecretsResolver)]
pub trait CanResolveSecrets<Config>: HasErrorType {
    /// Resolves the references in `config`.
    ///
    /// # Errors
    ///
    /// A denied or failing reference.
    fn resolve_secrets(&self, config: &mut Config) -> Result<(), Self::Error>;
}
