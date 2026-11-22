//! [`SecretRef`]: a secret reference (`env:NAME`, `file:name`, `docker:name`).
//!
//! The value is not a secret. `Debug` and `Display` show a [`RedactedRef`]: the scheme
//! and, when the rest is a single safe name, that name. Anything else (a path that
//! climbs out of a root, a query string, a value that could itself be a password) is
//! an FNV-1a digest. The raw rest is zeroized on drop.

use alloc::string::String;
use core::fmt;

use zeroize::Zeroize;

use crate::RedactedRef;

/// A parsed `scheme:rest` reference. The rest is zeroized on drop.
///
/// ```
/// use cgp_config_core::SecretRef;
///
/// let reference = SecretRef::parse("env:DB_PASSWORD").unwrap();
/// assert_eq!(reference.scheme(), "env");
/// assert_eq!(reference.to_string(), "env:DB_PASSWORD");
/// assert!(SecretRef::parse("hunter2").is_none());
/// let hostile = SecretRef::parse("vault:kv?token=hunter2").unwrap();
/// assert!(!hostile.to_string().contains("hunter2"));
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct SecretRef {
    scheme: String,
    rest: String,
}

impl Drop for SecretRef {
    fn drop(&mut self) {
        self.scheme.zeroize();
        self.rest.zeroize();
    }
}

impl SecretRef {
    /// Parses `scheme:rest`. The scheme is ASCII letters, digits, `+`, `-`, and `.`,
    /// and is stored lowercased. The rest must be non-empty. A string with no scheme
    /// is not a reference (a plain secret is just text).
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (scheme, rest) = text.split_once(':')?;
        if rest.is_empty() || !is_scheme(scheme) {
            return None;
        }
        Some(Self {
            scheme: scheme.to_ascii_lowercase(),
            rest: String::from(rest),
        })
    }

    /// The scheme, lowercased (`env`, `file`, `docker`, ...).
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// The rest of the reference. Resolvers read this; formatting never does.
    #[must_use]
    pub fn rest(&self) -> &str {
        &self.rest
    }

    /// Whether the rest is one safe path segment (`db`, `api_key`): no slash, not `.` or `..`.
    #[must_use]
    pub fn is_single_name(&self) -> bool {
        safe_name(&self.rest)
    }

    /// The form that is safe to put in an error or a log line.
    #[must_use]
    pub fn redacted(&self) -> RedactedRef {
        if matches!(self.scheme.as_str(), "env" | "file" | "docker") && safe_name(&self.rest) {
            RedactedRef::with_name(&self.scheme, &self.rest)
        } else {
            RedactedRef::digested(&self.scheme, &self.rest)
        }
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.redacted(), f)
    }
}

impl fmt::Debug for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretRef({self})")
    }
}

fn is_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// One path segment that can be shown: no slash, no `..`, no empty name.
pub(crate) fn safe_name(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::SecretRef;

    #[test]
    fn display_keeps_safe_names_and_hashes_the_rest() {
        assert_eq!(SecretRef::parse("FILE:db").unwrap().to_string(), "file:db");
        assert_eq!(
            SecretRef::parse("docker:api_key").unwrap().to_string(),
            "docker:api_key"
        );
        let climbed = SecretRef::parse("file:../secret").unwrap().to_string();
        assert!(climbed.starts_with("file:<redacted #"), "{climbed}");
        assert!(!climbed.contains(".."), "{climbed}");
        assert!(SecretRef::parse("nocolon").is_none());
        assert!(SecretRef::parse(":noname").is_none());
        assert!(SecretRef::parse("1abc:x").is_none());
    }
}
