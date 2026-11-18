use alloc::sync::Arc;
use core::fmt;

use indexmap::IndexMap;

use crate::ConfigValue;

type Hasher = foldhash::fast::FixedState;

/// An insertion-ordered map from keys to values.
///
/// Order is stable, so diffs, dumps, and templates are deterministic. Keys are `Arc<str>`,
/// so cloning a map clones reference counts, not strings. Equality ignores order, like a
/// TOML table or JSON object.
#[derive(Clone, Default, PartialEq)]
pub struct ConfigMap {
    entries: IndexMap<Arc<str>, ConfigValue, Hasher>,
}

impl ConfigMap {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty map with room for `capacity` entries.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: IndexMap::with_capacity_and_hasher(capacity, Hasher::default()),
        }
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The value under `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.entries.get(key)
    }

    /// The value under `key`, mutably.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut ConfigValue> {
        self.entries.get_mut(key)
    }

    /// Whether `key` is present.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Inserts `value` under `key`, returning the previous value. A new key goes last; an
    /// existing key keeps its position.
    pub fn insert(&mut self, key: impl Into<Arc<str>>, value: ConfigValue) -> Option<ConfigValue> {
        self.entries.insert(key.into(), value)
    }

    /// Removes `key`, keeping the order of the other entries.
    pub fn remove(&mut self, key: &str) -> Option<ConfigValue> {
        self.entries.shift_remove(key)
    }

    /// Iterates over the entries in order.
    pub fn iter(
        &self,
    ) -> impl ExactSizeIterator<Item = (&str, &ConfigValue)> + DoubleEndedIterator {
        self.entries.iter().map(|(key, value)| (&**key, value))
    }

    /// Iterates over the entries in order, with the shared key.
    pub fn iter_shared(&self) -> impl ExactSizeIterator<Item = (&Arc<str>, &ConfigValue)> {
        self.entries.iter()
    }

    pub(crate) fn raw_iter(&self) -> indexmap::map::Iter<'_, Arc<str>, ConfigValue> {
        self.entries.iter()
    }

    /// Iterates over the keys in order.
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str> {
        self.entries.keys().map(|key| &**key)
    }

    /// Iterates over the values in order.
    pub fn values(&self) -> impl ExactSizeIterator<Item = &ConfigValue> {
        self.entries.values()
    }

    /// Iterates over the values in order, mutably.
    pub fn values_mut(&mut self) -> impl ExactSizeIterator<Item = &mut ConfigValue> {
        self.entries.values_mut()
    }
}

impl fmt::Debug for ConfigMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<K: Into<Arc<str>>> FromIterator<(K, ConfigValue)> for ConfigMap {
    fn from_iter<I: IntoIterator<Item = (K, ConfigValue)>>(iter: I) -> Self {
        let mut map = Self::new();
        map.extend(iter);
        map
    }
}

impl<K: Into<Arc<str>>> Extend<(K, ConfigValue)> for ConfigMap {
    fn extend<I: IntoIterator<Item = (K, ConfigValue)>>(&mut self, iter: I) {
        for (key, value) in iter {
            self.insert(key, value);
        }
    }
}

impl IntoIterator for ConfigMap {
    type Item = (Arc<str>, ConfigValue);
    type IntoIter = indexmap::map::IntoIter<Arc<str>, ConfigValue>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::vec::Vec;

    use super::ConfigMap;
    use crate::ConfigValue;

    #[test]
    fn keeps_insertion_order_and_compares_unordered() {
        let mut map = ConfigMap::with_capacity(2);
        assert!(map.is_empty());
        map.insert("b", ConfigValue::from(1));
        map.insert("a", ConfigValue::from(2));
        assert_eq!(
            map.insert("b", ConfigValue::from(3)),
            Some(ConfigValue::from(1))
        );
        assert_eq!(map.keys().collect::<Vec<_>>(), ["b", "a"]);
        let reversed: ConfigMap = [("a", ConfigValue::from(2)), ("b", ConfigValue::from(3))]
            .into_iter()
            .collect();
        assert_eq!(map, reversed);
        assert!(map.contains_key("a"));
        assert_eq!(map.len(), 2);
        assert_eq!(map.values().count(), 2);
        assert_eq!(map.iter().next_back().map(|(k, _)| k), Some("a"));
        assert_eq!(map.iter_shared().count(), 2);
        *map.get_mut("a").unwrap() = ConfigValue::from(9);
        for value in map.values_mut() {
            assert!(value.as_i64().is_some());
        }
        assert_eq!(map.get("a").and_then(ConfigValue::as_i64), Some(9));
        assert_eq!(map.remove("b").and_then(|v| v.as_i64()), Some(3));
        assert_eq!(format!("{map:?}"), "{\"a\": 9}");
        assert_eq!(map.into_iter().count(), 1);
    }
}
