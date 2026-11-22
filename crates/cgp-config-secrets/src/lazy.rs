//! [`LazySecret<T>`]: resolved on first [`expose_secret`](LazySecret::expose_secret).

use alloc::sync::Arc;
use core::fmt;
use core::str::FromStr;

use cgp_config_core::{
    ConfigError, SecretReader, SecretRef, current_secret_reader, read_secret_reference,
};
use serde::de::{Deserializer, Visitor};
use zeroize::Zeroize;

use crate::secret::read_reference;
use crate::{REDACTED, Secret};

/// A secret resolved the first time it is exposed.
///
/// A literal is parsed during deserialize. A reference is stored, along with the reader
/// installed on the thread at that moment, and resolved on the first
/// [`expose_secret`](Self::expose_secret). Concurrent first calls may resolve twice; the
/// loser is dropped and zeroized. There is no TTL: a reload replaces the whole config.
pub struct LazySecret<T: Zeroize> {
    state: Arc<State<T>>,
}

struct State<T: Zeroize> {
    resolved: std::sync::OnceLock<Secret<T>>,
    pending: Option<Pending>,
}

struct Pending {
    reference: SecretRef,
    reader: Option<Arc<dyn SecretReader>>,
}

impl<T: Zeroize> Clone for LazySecret<T> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

impl<T: Zeroize> fmt::Debug for LazySecret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl<T: Zeroize> fmt::Display for LazySecret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl<T> LazySecret<T>
where
    T: Zeroize + FromStr + Clone,
{
    /// The secret, resolving a reference on the first call.
    ///
    /// # Errors
    ///
    /// The reference is refused or missing, or the text is not a `T`.
    pub fn expose_secret(&self) -> Result<&T, ConfigError> {
        Ok(self.secret()?.expose_secret())
    }

    fn secret(&self) -> Result<&Secret<T>, ConfigError> {
        if let Some(secret) = self.state.resolved.get() {
            return Ok(secret);
        }
        let pending = self
            .state
            .pending
            .as_ref()
            .ok_or(ConfigError::SecretsDenied)?;
        let text = match &pending.reader {
            Some(reader) => reader.read_secret(&pending.reference)?,
            None => read_secret_reference(&pending.reference)?,
        };
        let secret = Secret::parse_exposed(text.expose()).ok_or_else(|| {
            ConfigError::type_error(
                cgp_config_core::KeyPath::root(),
                "invalid secret value",
                None,
            )
        })?;
        Ok(self.state.resolved.get_or_init(|| secret))
    }
}

impl<'de, T> serde::Deserialize<'de> for LazySecret<T>
where
    T: Zeroize + FromStr + Clone,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(LazyVisitor {
            kind: core::marker::PhantomData,
        })
    }
}

struct LazyVisitor<T> {
    kind: core::marker::PhantomData<T>,
}

impl<'de, T> Visitor<'de> for LazyVisitor<T>
where
    T: Zeroize + FromStr + Clone,
{
    type Value = LazySecret<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a secret string or a { secret = \"scheme:name\" } table")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Self::Value, E> {
        let secret =
            Secret::parse_exposed(text).ok_or_else(|| E::custom("invalid secret value"))?;
        Ok(ready(secret))
    }

    fn visit_string<E: serde::de::Error>(self, mut text: String) -> Result<Self::Value, E> {
        let secret = self.visit_str(&text);
        text.zeroize();
        secret
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        let text = read_reference(map)?;
        let reference = SecretRef::parse(text.expose())
            .ok_or_else(|| serde::de::Error::custom("invalid secret value"))?;
        Ok(deferred(reference))
    }
}

fn ready<T: Zeroize>(secret: Secret<T>) -> LazySecret<T> {
    let resolved = std::sync::OnceLock::new();
    let _ = resolved.set(secret);
    LazySecret {
        state: Arc::new(State {
            resolved,
            pending: None,
        }),
    }
}

fn deferred<T: Zeroize>(reference: SecretRef) -> LazySecret<T> {
    LazySecret {
        state: Arc::new(State {
            resolved: std::sync::OnceLock::new(),
            pending: Some(Pending {
                reference,
                reader: current_secret_reader(),
            }),
        }),
    }
}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;

    use cgp_config_core::{ConfigError, SecretGuard, SecretReader, SecretRef, SecretText};

    use super::LazySecret;
    use crate::Secret;

    struct Fixed;

    impl SecretReader for Fixed {
        fn read_secret(&self, reference: &SecretRef) -> Result<SecretText, ConfigError> {
            assert_eq!(reference.rest(), "TOKEN");
            Ok(SecretText::new("s3cr3t-SENTINEL"))
        }
    }

    #[test]
    fn a_reference_resolves_once_and_stays_redacted() {
        let parsed: LazySecret<String> = serde_json::from_value(serde_json::json!({
            "secret": "env:TOKEN"
        }))
        .unwrap();
        assert_eq!(alloc::format!("{parsed:?}"), "[REDACTED]");
        let _guard = SecretGuard::resolve(Arc::new(Fixed));
        // The reader was captured at deserialize, before this guard. Re-parse under it.
        drop(_guard);
        let guard = SecretGuard::resolve(Arc::new(Fixed));
        let parsed: LazySecret<String> = serde_json::from_value(serde_json::json!({
            "secret": "env:TOKEN"
        }))
        .unwrap();
        drop(guard);
        assert_eq!(parsed.expose_secret().unwrap(), "s3cr3t-SENTINEL");
        assert_eq!(parsed.expose_secret().unwrap(), "s3cr3t-SENTINEL");
        let literal: LazySecret<String> =
            serde_json::from_value(serde_json::json!("s3cr3t-SENTINEL")).unwrap();
        assert_eq!(literal.expose_secret().unwrap(), "s3cr3t-SENTINEL");
        assert!(!alloc::format!("{literal:?}").contains("SENTINEL"));
        let _ = Secret::<String>::new(String::new());
    }
}
