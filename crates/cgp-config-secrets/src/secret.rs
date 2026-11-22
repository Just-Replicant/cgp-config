//! [`Secret<T>`]: an owned secret. `Debug` and `Display` are `[REDACTED]`.

use alloc::sync::Arc;
use core::fmt;
use core::str::FromStr;

use cgp_config_core::{ConfigError, SecretRef, SecretText, read_secret_reference};
use secrecy::{ExposeSecret, SecretBox};
use serde::de::{self, Deserializer, MapAccess, Visitor};
use zeroize::Zeroize;

use crate::REDACTED;

/// A secret of type `T`.
///
/// Clones share one allocation. The bytes are zeroized when the last clone is dropped.
/// `T: Send + Sync` makes `Secret<T>` both.
///
/// Deserialize accepts a string (a literal, already sensitive) or a one-key table
/// `{ secret = "env:NAME" }` / `file:` / `docker:`. A reference is resolved from the
/// session active on this thread. There is no `Serialize` impl; use
/// [`serialize_redacted`] on a field that must be written as `[REDACTED]`.
pub struct Secret<T: Zeroize> {
    inner: Arc<SecretBox<T>>,
}

impl<T: Zeroize> Secret<T> {
    /// Wraps `value`. The caller should not keep another copy.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(SecretBox::new(Box::new(value))),
        }
    }

    /// The secret. The borrow lasts only as long as this value.
    #[must_use]
    pub fn expose_secret(&self) -> &T {
        self.inner.expose_secret()
    }
}

impl Secret<String> {
    /// Moves redacted text into a `Secret<String>` and zeroizes the buffer.
    #[must_use]
    pub fn from_text(text: SecretText) -> Self {
        let secret = Self::new(String::from(text.expose()));
        drop(text);
        secret
    }
}

impl<T> Secret<T>
where
    T: Zeroize + FromStr,
{
    /// Parses exposed text. `None` does not include `text`.
    #[must_use]
    pub fn parse_exposed(text: &str) -> Option<Self> {
        text.parse().ok().map(Self::new)
    }
}

impl<T: Zeroize> ExposeSecret<T> for Secret<T> {
    fn expose_secret(&self) -> &T {
        Secret::expose_secret(self)
    }
}

impl<T: Zeroize> Clone for Secret<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: Zeroize + PartialEq> PartialEq for Secret<T> {
    fn eq(&self, other: &Self) -> bool {
        self.expose_secret() == other.expose_secret()
    }
}

impl<T: Zeroize + Eq> Eq for Secret<T> {}

impl<T: Zeroize> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl<T: Zeroize> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// Writes `[REDACTED]`, for a field that opts in with
/// `#[serde(serialize_with = "serialize_redacted")]`.
///
/// # Errors
///
/// The serializer's error.
pub fn serialize_redacted<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let _ = value;
    serializer.serialize_str(REDACTED)
}

impl<'de, T> serde::Deserialize<'de> for Secret<T>
where
    T: Zeroize + FromStr,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SecretVisitor {
            kind: core::marker::PhantomData,
        })
    }
}

struct SecretVisitor<T> {
    kind: core::marker::PhantomData<T>,
}

impl<'de, T> Visitor<'de> for SecretVisitor<T>
where
    T: Zeroize + FromStr,
{
    type Value = Secret<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a secret string or a { secret = \"scheme:name\" } table")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Self::Value, E> {
        Secret::parse_exposed(text).ok_or_else(|| E::custom("invalid secret value"))
    }

    fn visit_string<E: de::Error>(self, mut text: String) -> Result<Self::Value, E> {
        let secret = self.visit_str(&text);
        text.zeroize();
        secret
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        let text = read_reference(map)?;
        let reference = match SecretRef::parse(text.expose()) {
            Some(reference) => reference,
            None => return Err(de::Error::custom("invalid secret value")),
        };
        let resolved = read_secret_reference(&reference).map_err(session_error::<A::Error>)?;
        Secret::parse_exposed(resolved.expose())
            .ok_or_else(|| de::Error::custom("invalid secret value"))
    }
}

pub(crate) fn read_reference<'de, A: MapAccess<'de>>(mut map: A) -> Result<SecretText, A::Error> {
    let mut reference = None;
    while let Some(key) = map.next_key::<String>()? {
        if key == "secret" && reference.is_none() {
            let mut value = map.next_value::<String>()?;
            reference = Some(SecretText::new(value.as_str()));
            value.zeroize();
        } else {
            return Err(de::Error::custom("a secret table has one key, `secret`"));
        }
    }
    reference.ok_or_else(|| de::Error::custom("a secret table needs a `secret` key"))
}

pub(crate) fn session_error<E: de::Error>(error: ConfigError) -> E {
    match cgp_config_core::secret_session_message(&error) {
        Some(message) => E::custom(message),
        None => E::custom("invalid secret value"),
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;

    #[test]
    fn debug_display_and_serialize_hide_the_secret() {
        let secret = Secret::new(String::from("s3cr3t-SENTINEL"));
        assert_eq!(alloc::format!("{secret:?}"), "[REDACTED]");
        assert_eq!(alloc::format!("{secret}"), "[REDACTED]");
        assert_eq!(secret.expose_secret(), "s3cr3t-SENTINEL");
        let json = serde_json::to_string(&serde_json::json!(null)).unwrap();
        let _ = json;
        let redacted = super::serialize_redacted(&secret, serde_json::value::Serializer).unwrap();
        assert_eq!(
            redacted,
            serde_json::Value::String(String::from("[REDACTED]"))
        );
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Secret<String>>();
    }
}
