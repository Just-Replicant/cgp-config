//! [`EnvSnapshot`]: the process environment, read once at bootstrap.
//!
//! Nothing in cgp-config reads `std::env` after bootstrap and nothing writes it. Every
//! provider that needs a variable reads the snapshot through [`HasEnvSnapshot`], so tests
//! build a snapshot by hand and stay hermetic and parallel.

use alloc::collections::{BTreeMap, btree_map};
use alloc::string::String;
use core::fmt;

use cgp_fork::prelude::*;

/// An owned copy of environment variables.
///
/// `Debug` prints the variable names only, never their values.
///
/// ```
/// use cgp_config_core::EnvSnapshot;
///
/// let env = EnvSnapshot::from_iter([("APP__HTTP__PORT", "8080"), ("HOME", "/root")]);
/// assert_eq!(env.get("APP__HTTP__PORT"), Some("8080"));
/// assert_eq!(format!("{env:?}"), r#"EnvSnapshot(["APP__HTTP__PORT", "HOME"])"#);
/// ```
#[derive(Clone, Default, PartialEq, Eq)]
pub struct EnvSnapshot {
    vars: BTreeMap<String, String>,
}

impl EnvSnapshot {
    /// An empty snapshot.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The value of `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    /// Whether `key` is set.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.vars.contains_key(key)
    }

    /// Sets `key` (in the snapshot only; the process environment is never written).
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }

    /// Removes and returns `key`.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.vars.remove(key)
    }

    /// Number of variables.
    #[must_use]
    pub fn len(&self) -> usize {
        self.vars.len()
    }

    /// Whether the snapshot is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    /// The variables, sorted by name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.vars.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// The variables whose name starts with `prefix`, sorted by name.
    pub fn with_prefix<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> {
        self.vars
            .range::<str, _>((
                core::ops::Bound::Included(prefix),
                core::ops::Bound::Unbounded,
            ))
            .take_while(move |(key, _)| key.starts_with(prefix))
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

impl<K: Into<String>, V: Into<String>> FromIterator<(K, V)> for EnvSnapshot {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self {
            vars: iter
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }
}

impl<K: Into<String>, V: Into<String>> Extend<(K, V)> for EnvSnapshot {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        self.vars
            .extend(iter.into_iter().map(|(k, v)| (k.into(), v.into())));
    }
}

impl IntoIterator for EnvSnapshot {
    type Item = (String, String);
    type IntoIter = btree_map::IntoIter<String, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.vars.into_iter()
    }
}

impl fmt::Debug for EnvSnapshot {
    /// Names only; values may be secrets.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("EnvSnapshot")
            .field(&self.vars.keys().collect::<alloc::vec::Vec<_>>())
            .finish()
    }
}

/// The environment snapshot of a loader. Implemented automatically for a context with an
/// `env_snapshot: EnvSnapshot` field.
#[cgp_auto_getter]
pub trait HasEnvSnapshot {
    /// The snapshot.
    fn env_snapshot(&self) -> &EnvSnapshot;
}

/// The application's env prefix (`APP`, no trailing underscore). Nested keys are
/// `{PREFIX}__{PATH}`. Implemented automatically for a context with an
/// `env_prefix: String` field.
#[cgp_auto_getter]
pub trait HasEnvPrefix {
    /// The prefix.
    fn env_prefix(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::vec::Vec;

    use super::EnvSnapshot;

    #[test]
    fn prefix_scan_insert_remove() {
        let mut env = EnvSnapshot::from_iter([("APP__A", "1"), ("APP_B", "2"), ("APQ", "3")]);
        env.extend([("APP__C", "4")]);
        let keys: Vec<_> = env.with_prefix("APP__").map(|(k, _)| k).collect();
        assert_eq!(keys, ["APP__A", "APP__C"]);
        assert_eq!(env.remove("APP_B").as_deref(), Some("2"));
        env.insert("Z", "z");
        assert!(env.contains("Z"));
        assert_eq!(env.len(), 4);
        assert!(!env.is_empty());
        assert!(EnvSnapshot::new().is_empty());
        assert!(!format!("{env:?}").contains('4'));
        assert_eq!(env.iter().count(), 4);
        assert_eq!(env.into_iter().count(), 4);
    }
}
