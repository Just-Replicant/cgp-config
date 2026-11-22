//! The secret session active while a config is deserialized.
//!
//! `Secret<T>` reads it from `deserialize`. With no resolving session every reference
//! fails with [`ConfigError::SecretsDenied`]. A resolving session (the `secrets`
//! feature) fills the secret from its allowlisted schemes. The guard restores the
//! previous session on drop, including on panic.
//!
//! The session is thread-local and only exists with `std`. Deserialization is
//! synchronous and runs on the thread that opened the guard.

use alloc::string::String;
use core::fmt;

use zeroize::Zeroizing;

use crate::{ConfigError, SecretRef};

/// Secret text. `Debug` is `[REDACTED]`. The bytes are zeroized on drop.
pub struct SecretText(Zeroizing<String>);

impl SecretText {
    /// Wraps `text`. The previous owner should not keep a copy.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(Zeroizing::new(text.into()))
    }

    /// The secret. The borrow lasts only as long as this value.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Hands the buffer to a container that zeroizes on its own drop.
    #[must_use]
    pub fn into_zeroizing(self) -> Zeroizing<String> {
        self.0
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// Reads one secret reference. Object-safe so a session can hold it without naming
/// the loader. Errors must not include the secret.
#[cfg(feature = "std")]
pub trait SecretReader: Send + Sync {
    /// Resolves `reference`.
    ///
    /// # Errors
    ///
    /// A refused reference, a missing source, or a read failure.
    fn read_secret(&self, reference: &SecretRef) -> Result<SecretText, ConfigError>;
}

/// Restores the previous secret session when dropped.
pub struct SecretGuard {
    #[cfg(feature = "std")]
    previous: Option<alloc::sync::Arc<dyn SecretReader>>,
}

impl fmt::Debug for SecretGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = self;
        f.write_str("SecretGuard")
    }
}

impl SecretGuard {
    /// A session that refuses every reference.
    #[must_use]
    pub fn deny() -> Self {
        #[cfg(feature = "std")]
        {
            Self::install(None)
        }
        #[cfg(not(feature = "std"))]
        {
            Self {}
        }
    }

    /// A session that resolves references with `reader`.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn resolve(reader: alloc::sync::Arc<dyn SecretReader>) -> Self {
        Self::install(Some(reader))
    }

    #[cfg(feature = "std")]
    fn install(next: Option<alloc::sync::Arc<dyn SecretReader>>) -> Self {
        let previous = SESSION.with(|cell| {
            let mut slot = cell.borrow_mut();
            core::mem::replace(&mut *slot, next)
        });
        Self { previous }
    }
}

#[cfg(feature = "std")]
impl Drop for SecretGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        SESSION.with(|cell| *cell.borrow_mut() = previous);
    }
}

/// The reader installed on this thread, if the session resolves secrets.
#[cfg(feature = "std")]
#[must_use]
pub fn current_secret_reader() -> Option<alloc::sync::Arc<dyn SecretReader>> {
    SESSION.with(|cell| cell.borrow().clone())
}

/// Resolves `reference` with the session on this thread.
///
/// # Errors
///
/// [`ConfigError::SecretsDenied`] when no resolving session is installed, or the
/// reader's error. Without `std` every reference is denied.
pub fn read_secret_reference(reference: &SecretRef) -> Result<SecretText, ConfigError> {
    #[cfg(feature = "std")]
    {
        let reader = SESSION.with(|cell| cell.borrow().clone());
        match reader {
            Some(reader) => reader.read_secret(reference),
            None => Err(ConfigError::SecretsDenied),
        }
    }
    #[cfg(not(feature = "std"))]
    {
        let _ = reference;
        Err(ConfigError::SecretsDenied)
    }
}

#[cfg(feature = "std")]
std::thread_local! {
    static SESSION: core::cell::RefCell<Option<alloc::sync::Arc<dyn SecretReader>>> =
        const { core::cell::RefCell::new(None) };
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use alloc::string::String;
    use alloc::sync::Arc;

    use super::{SecretGuard, SecretReader, SecretText, read_secret_reference};
    use crate::{ConfigError, SecretRef};

    struct Fixed;

    impl SecretReader for Fixed {
        fn read_secret(&self, reference: &SecretRef) -> Result<SecretText, ConfigError> {
            assert_eq!(reference.scheme(), "env");
            Ok(SecretText::new(String::from("s3cr3t-SENTINEL")))
        }
    }

    #[test]
    fn the_default_session_denies_and_a_guard_restores_it() {
        let reference = SecretRef::parse("env:TOKEN").unwrap();
        assert_eq!(
            read_secret_reference(&reference).unwrap_err(),
            ConfigError::SecretsDenied
        );
        let text = {
            let _guard = SecretGuard::resolve(Arc::new(Fixed));
            let text = read_secret_reference(&reference).unwrap();
            assert_eq!(text.expose(), "s3cr3t-SENTINEL");
            assert_eq!(alloc::format!("{text:?}"), "[REDACTED]");
            text
        };
        assert_eq!(
            read_secret_reference(&reference).unwrap_err(),
            ConfigError::SecretsDenied
        );
        assert_eq!(text.expose(), "s3cr3t-SENTINEL");
    }
}
