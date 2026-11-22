//! Per-scheme resolution. The session calls the same functions the providers call.

use cgp_config_core::{SecretRef, SecretText};
use cgp_fork::prelude::*;

use crate::{Secret, SecretRoots};

/// Marker for the `env:` scheme.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvScheme;

/// Marker for the `file:` scheme.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileScheme;

/// Marker for the `docker:` scheme.
#[derive(Debug, Clone, Copy, Default)]
pub struct DockerScheme;

/// Resolves one scheme into a `Secret<String>`.
///
/// `UseDelegate<StdSecretSchemes>` wires `env:`, `file:`, and `docker:`.
#[cgp_component {
    name: SecretResolverComponent,
    provider: SecretResolver,
}]
#[derive_delegate(UseDelegate<Scheme>)]
pub trait CanResolveSecret<Scheme>: HasErrorType {
    /// Resolves `reference` with this scheme's provider.
    ///
    /// # Errors
    ///
    /// The reference is refused, missing, or not valid for the scheme.
    fn resolve_secret(&self, reference: &SecretRef) -> Result<Secret<String>, Self::Error>;
}

/// Async twin of [`CanResolveSecret`]. 0.1 ships no provider; satellite crates implement
/// it for `vault:`, `aws:`, and `gcp:`.
#[cgp_component {
    name: AsyncSecretResolverComponent,
    provider: AsyncSecretResolver,
}]
#[derive_delegate(UseDelegate<Scheme>)]
#[cgp_async(Send)]
pub trait CanResolveSecretAsync<Scheme>: HasErrorType {
    /// Resolves `reference`.
    ///
    /// # Errors
    ///
    /// The reference is refused, missing, or not valid for the scheme.
    async fn resolve_secret_async(&self, reference: &SecretRef) -> Result<SecretText, Self::Error>;
}

/// The directories a `file:` reference is allowed to name.
#[cgp_component {
    name: SecretRootsComponent,
    provider: SecretRootsGetter,
}]
pub trait HasSecretRoots {
    /// The allowlisted directories, canonicalized.
    fn secret_roots(&self) -> SecretRoots;
}
