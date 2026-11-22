//! An opt-in secret map for tests. [`Debug`](SecretMap) prints names, never values.
//!
//! It is not part of [`TestConfigPreset`](cgp_config_core): that preset is the default of
//! every in-memory loader, and a secret field there would force every test context to
//! carry one. Contexts that want stand-ins use [`MockSecretsPreset`] in the providers
//! crate, which reads a `secret_map` field.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use core::fmt;

use cgp_config_core::{
    ConfigError, SecretGuard, SecretReader, SecretRef, SecretSessionComponent, SecretSessionOpener,
    SecretText,
};
use cgp_fork::prelude::*;
use zeroize::Zeroizing;

use crate::Secret;
use crate::component::{
    DockerScheme, EnvScheme, FileScheme, SecretResolver, SecretResolverComponent,
};

/// Names mapped to secret text. The key is the reference as written (`env:TOKEN`).
#[derive(Clone, Default)]
pub struct SecretMap {
    values: BTreeMap<String, Zeroizing<String>>,
}

impl SecretMap {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts `reference` (`env:TOKEN`, `file:db`, ...).
    pub fn insert(&mut self, reference: impl AsRef<str>, value: impl Into<String>) -> &mut Self {
        self.values.insert(
            String::from(reference.as_ref()),
            Zeroizing::new(value.into()),
        );
        self
    }

    fn text(&self, reference: &SecretRef) -> Result<SecretText, ConfigError> {
        let key = alloc::format!("{}:{}", reference.scheme(), reference.rest());
        match self.values.get(&key) {
            Some(value) => Ok(SecretText::new(value.as_str())),
            None => Err(ConfigError::SecretDenied {
                reference: reference.redacted(),
            }),
        }
    }
}

impl fmt::Debug for SecretMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self
            .values
            .keys()
            .map(|key| match SecretRef::parse(key) {
                Some(reference) => alloc::format!("{}", reference.redacted()),
                None => String::from("<unparsed>"),
            })
            .collect();
        f.debug_struct("SecretMap").field("names", &names).finish()
    }
}

struct MapReader(SecretMap);

impl SecretReader for MapReader {
    fn read_secret(&self, reference: &SecretRef) -> Result<SecretText, ConfigError> {
        self.0.text(reference)
    }
}

/// Opens a session that reads the context's `secret_map` field.
pub struct OpenMockSession;

#[cgp_impl(OpenMockSession: SecretSessionComponent)]
impl SecretSessionOpener
where
    Self: HasField<Symbol!("secret_map"), Value = SecretMap>,
{
    fn open_secret_session(&self) -> SecretGuard {
        let map = self
            .get_field(core::marker::PhantomData::<Symbol!("secret_map")>)
            .clone();
        SecretGuard::resolve(Arc::new(MapReader(map)))
    }
}

/// Resolves every built-in scheme from the context's `secret_map` field.
pub struct MockSecret;

#[cgp_impl(MockSecret: SecretResolverComponent)]
impl<Scheme> SecretResolver<Scheme>
where
    Self: HasField<Symbol!("secret_map"), Value = SecretMap> + CanRaiseError<ConfigError>,
{
    fn resolve_secret(&self, reference: &SecretRef) -> Result<Secret<String>, Self::Error> {
        self.get_field(core::marker::PhantomData::<Symbol!("secret_map")>)
            .text(reference)
            .map(Secret::from_text)
            .map_err(Self::raise_error)
    }
}

/// `env:`, `file:`, and `docker:` backed by [`MockSecret`].
pub struct MockSecretSchemes;

delegate_components! {
    #[unchecked]
    MockSecretSchemes {
        [EnvScheme, FileScheme, DockerScheme]: MockSecret,
    }
}

#[cfg(test)]
mod tests {
    use super::SecretMap;

    #[test]
    fn debug_lists_names_only() {
        let mut map = SecretMap::new();
        map.insert("env:TOKEN", "s3cr3t-SENTINEL");
        let shown = alloc::format!("{map:?}");
        assert!(shown.contains("env:TOKEN"), "{shown}");
        assert!(!shown.contains("SENTINEL"), "{shown}");
    }
}
