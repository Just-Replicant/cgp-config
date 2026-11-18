use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt;

use cgp_config_core::{ConfigError, KeyPath, LoadLimits, Meta, Origin, PathSegment};

use crate::{ConfigMap, Number};

/// The data of one node.
#[derive(Clone, PartialEq)]
#[non_exhaustive]
pub enum Kind {
    /// An explicit null. A null overlay stores null; it does not delete a lower value.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number.
    Number(Number),
    /// A string. Env and CLI layers store every value as a string; the deserializer
    /// parses it by the target type.
    String(Arc<str>),
    /// An array.
    Array(Vec<ConfigValue>),
    /// A map.
    Map(ConfigMap),
}

impl Kind {
    /// Short name of the kind: `null`, `bool`, `integer`, `float`, `string`, `array`, `map`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Number(number) => number.kind_name(),
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Map(_) => "map",
        }
    }
}

#[derive(Clone)]
struct Node {
    kind: Kind,
    meta: Meta,
}

/// The intermediate configuration tree: a node with its [`Kind`] and [`Meta`].
///
/// Nodes are reference-counted, so `clone` is one atomic increment and unchanged subtrees
/// are shared between layers, merges, and reloads. Mutation (`kind_mut`, `meta_mut`,
/// `as_map_mut`) copies a node only when it is shared (`Arc::make_mut`).
///
/// **Equality ignores metadata**: two trees are equal when their data is, wherever it came
/// from. [`eq_with_meta`](Self::eq_with_meta) compares both.
///
/// `Debug` prints `[REDACTED]` for nodes whose `Meta::secret` is set. The tree stores secret
/// *references* only; resolved secret bytes live in `Secret<T>` fields of the config struct.
///
/// ```
/// use cgp_config_value::ConfigValue;
///
/// let doc = ConfigValue::map([
///     ("name", ConfigValue::from("demo")),
///     ("http", ConfigValue::map([("port", ConfigValue::from(8080))])),
/// ]);
/// assert_eq!(doc.get("http").and_then(|h| h.get("port")).and_then(ConfigValue::as_u64), Some(8080));
/// assert_eq!(doc.get_path(&"http.port".into()).map(ConfigValue::kind_name), Some("integer"));
/// ```
#[derive(Clone)]
pub struct ConfigValue {
    node: Arc<Node>,
}

impl ConfigValue {
    /// A node from its data and metadata.
    #[must_use]
    pub fn new(kind: Kind, meta: Meta) -> Self {
        Self {
            node: Arc::new(Node { kind, meta }),
        }
    }

    /// A node with default metadata (origin unknown).
    #[must_use]
    pub fn from_kind(kind: Kind) -> Self {
        Self::new(kind, Meta::default())
    }

    /// An explicit null.
    #[must_use]
    pub fn null() -> Self {
        Self::from_kind(Kind::Null)
    }

    /// An empty map.
    #[must_use]
    pub fn empty_map() -> Self {
        Self::from_kind(Kind::Map(ConfigMap::new()))
    }

    /// A map from entries.
    #[must_use]
    pub fn map<K: Into<Arc<str>>>(entries: impl IntoIterator<Item = (K, Self)>) -> Self {
        Self::from_kind(Kind::Map(entries.into_iter().collect()))
    }

    /// An array from elements.
    #[must_use]
    pub fn array(elements: impl IntoIterator<Item = Self>) -> Self {
        Self::from_kind(Kind::Array(elements.into_iter().collect()))
    }

    /// The same data with `meta` replacing this node's metadata.
    #[must_use]
    pub fn with_meta(mut self, meta: Meta) -> Self {
        *self.meta_mut() = meta;
        self
    }

    /// The same data with this node's origin replaced (children keep theirs).
    #[must_use]
    pub fn with_origin(mut self, origin: Origin) -> Self {
        self.meta_mut().origin = origin;
        self
    }

    /// Sets the origin of this node and of every descendant to `origin`. A source calls this
    /// once on the layer it built.
    pub fn stamp_origin(&mut self, origin: &Origin) {
        self.meta_mut().origin = origin.clone();
        match self.kind_mut() {
            Kind::Array(elements) => {
                for element in elements {
                    element.stamp_origin(origin);
                }
            }
            Kind::Map(map) => {
                for value in map.values_mut() {
                    value.stamp_origin(origin);
                }
            }
            _ => {}
        }
    }

    /// The node's data.
    #[must_use]
    pub fn kind(&self) -> &Kind {
        &self.node.kind
    }

    /// The node's metadata.
    #[must_use]
    pub fn meta(&self) -> &Meta {
        &self.node.meta
    }

    /// The node's data, mutably (copies the node if it is shared).
    pub fn kind_mut(&mut self) -> &mut Kind {
        &mut Arc::make_mut(&mut self.node).kind
    }

    /// The node's metadata, mutably (copies the node if it is shared).
    pub fn meta_mut(&mut self) -> &mut Meta {
        &mut Arc::make_mut(&mut self.node).meta
    }

    /// Data and metadata, without copying when this is the only reference.
    #[must_use]
    pub fn into_parts(self) -> (Kind, Meta) {
        let node = Arc::unwrap_or_clone(self.node);
        (node.kind, node.meta)
    }

    /// Whether `a` and `b` are the same allocation (a shared, unchanged subtree).
    #[must_use]
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.node, &b.node)
    }

    /// Short kind name: `null`, `bool`, `integer`, `float`, `string`, `array`, `map`.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        self.kind().name()
    }

    /// Whether the node is null.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self.kind(), Kind::Null)
    }

    /// Whether the node is a map.
    #[must_use]
    pub fn is_map(&self) -> bool {
        matches!(self.kind(), Kind::Map(_))
    }

    /// Whether the node is an array.
    #[must_use]
    pub fn is_array(&self) -> bool {
        matches!(self.kind(), Kind::Array(_))
    }

    /// The boolean, when the node is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self.kind() {
            Kind::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The number, when the node is one.
    #[must_use]
    pub fn as_number(&self) -> Option<&Number> {
        match self.kind() {
            Kind::Number(number) => Some(number),
            _ => None,
        }
    }

    /// The value as `u64`, when the node is a non-negative integer.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        self.as_number().and_then(Number::as_u64)
    }

    /// The value as `i64`, when the node is an integer in range.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        self.as_number().and_then(Number::as_i64)
    }

    /// The value as `f64`, when the node is a number.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        self.as_number().map(Number::as_f64)
    }

    /// The string, when the node is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self.kind() {
            Kind::String(text) => Some(text),
            _ => None,
        }
    }

    /// The elements, when the node is an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Self]> {
        match self.kind() {
            Kind::Array(elements) => Some(elements),
            _ => None,
        }
    }

    /// The map, when the node is one.
    #[must_use]
    pub fn as_map(&self) -> Option<&ConfigMap> {
        match self.kind() {
            Kind::Map(map) => Some(map),
            _ => None,
        }
    }

    /// The map, mutably, when the node is one (copies the node if it is shared).
    pub fn as_map_mut(&mut self) -> Option<&mut ConfigMap> {
        match self.kind_mut() {
            Kind::Map(map) => Some(map),
            _ => None,
        }
    }

    /// The child under `key`, when the node is a map.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        self.as_map().and_then(|map| map.get(key))
    }

    /// The element at `index`, when the node is an array.
    #[must_use]
    pub fn get_index(&self, index: usize) -> Option<&Self> {
        self.as_array().and_then(|elements| elements.get(index))
    }

    /// The descendant at `path`.
    #[must_use]
    pub fn get_path(&self, path: &KeyPath) -> Option<&Self> {
        path.iter().try_fold(self, |node, segment| match segment {
            PathSegment::Key(key) => node.get(key),
            PathSegment::Index(index) => node.get_index(*index),
        })
    }

    /// Compares data *and* metadata, recursively.
    #[must_use]
    pub fn eq_with_meta(&self, other: &Self) -> bool {
        if Self::ptr_eq(self, other) {
            return true;
        }
        if self.meta() != other.meta() {
            return false;
        }
        match (self.kind(), other.kind()) {
            (Kind::Array(a), Kind::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_with_meta(y))
            }
            (Kind::Map(a), Kind::Map(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(key, x)| b.get(key).is_some_and(|y| x.eq_with_meta(y)))
            }
            (a, b) => a == b,
        }
    }

    /// Number of map entries plus array elements in the whole tree.
    #[must_use]
    pub fn count_entries(&self) -> usize {
        match self.kind() {
            Kind::Array(elements) => {
                elements.len() + elements.iter().map(Self::count_entries).sum::<usize>()
            }
            Kind::Map(map) => map.len() + map.values().map(Self::count_entries).sum::<usize>(),
            _ => 0,
        }
    }

    /// Nesting depth: 0 for a scalar or an empty container, 1 + the deepest child otherwise.
    #[must_use]
    pub fn depth(&self) -> usize {
        let children: &mut dyn Iterator<Item = &Self> = match self.kind() {
            Kind::Array(elements) => &mut elements.iter(),
            Kind::Map(map) => &mut map.values(),
            _ => return 0,
        };
        children.map(|child| child.depth() + 1).max().unwrap_or(0)
    }

    /// Checks `max_depth` and `max_keys` without walking further than the limits allow.
    ///
    /// # Errors
    ///
    /// `ConfigError::LimitExceeded` naming the first exceeded limit.
    pub fn check_limits(&self, limits: &LoadLimits) -> Result<(), ConfigError> {
        fn walk(
            node: &ConfigValue,
            depth: usize,
            keys: &mut usize,
            limits: &LoadLimits,
        ) -> Result<(), ConfigError> {
            let children: &mut dyn Iterator<Item = &ConfigValue> = match node.kind() {
                Kind::Array(elements) => &mut elements.iter(),
                Kind::Map(map) => &mut map.values(),
                _ => return Ok(()),
            };
            for child in children {
                if depth + 1 > limits.max_depth {
                    return Err(ConfigError::limit_exceeded("max_depth", limits.max_depth));
                }
                *keys += 1;
                if *keys > limits.max_keys {
                    return Err(ConfigError::limit_exceeded("max_keys", limits.max_keys));
                }
                walk(child, depth + 1, keys, limits)?;
            }
            Ok(())
        }
        let mut keys = 0;
        walk(self, 0, &mut keys, limits)
    }

    /// A copy in which every secret node is replaced by the string `"[REDACTED]"`, for
    /// dumps and `explain`.
    #[must_use]
    pub fn redacted(&self) -> Self {
        if self.meta().secret {
            return Self::new(
                Kind::String(Arc::from(crate::REDACTED)),
                self.meta().clone(),
            );
        }
        match self.kind() {
            Kind::Array(elements) => {
                let elements = elements.iter().map(Self::redacted).collect();
                Self::new(Kind::Array(elements), self.meta().clone())
            }
            Kind::Map(map) => {
                let map = map
                    .iter_shared()
                    .map(|(k, v)| (k.clone(), v.redacted()))
                    .collect();
                Self::new(Kind::Map(map), self.meta().clone())
            }
            _ => self.clone(),
        }
    }
}

impl Default for ConfigValue {
    /// An empty map, the identity of every merge.
    fn default() -> Self {
        Self::empty_map()
    }
}

impl PartialEq for ConfigValue {
    fn eq(&self, other: &Self) -> bool {
        Self::ptr_eq(self, other) || self.kind() == other.kind()
    }
}

impl fmt::Debug for ConfigValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.meta().secret {
            return f.write_str(crate::REDACTED);
        }
        match self.kind() {
            Kind::Null => f.write_str("null"),
            Kind::Bool(value) => write!(f, "{value}"),
            Kind::Number(number) => write!(f, "{number}"),
            Kind::String(text) => write!(f, "{text:?}"),
            Kind::Array(elements) => f.debug_list().entries(elements).finish(),
            Kind::Map(map) => fmt::Debug::fmt(map, f),
        }
    }
}

impl From<bool> for ConfigValue {
    fn from(value: bool) -> Self {
        Self::from_kind(Kind::Bool(value))
    }
}

impl From<Number> for ConfigValue {
    fn from(value: Number) -> Self {
        Self::from_kind(Kind::Number(value))
    }
}

macro_rules! from_number {
    ($($t:ty),*) => {$(
        impl From<$t> for ConfigValue {
            fn from(value: $t) -> Self {
                Self::from(Number::from(value))
            }
        }
    )*};
}

from_number!(u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);

impl From<&str> for ConfigValue {
    fn from(value: &str) -> Self {
        Self::from_kind(Kind::String(Arc::from(value)))
    }
}

impl From<String> for ConfigValue {
    fn from(value: String) -> Self {
        Self::from_kind(Kind::String(Arc::from(value)))
    }
}

impl From<Arc<str>> for ConfigValue {
    fn from(value: Arc<str>) -> Self {
        Self::from_kind(Kind::String(value))
    }
}

impl From<Vec<Self>> for ConfigValue {
    fn from(value: Vec<Self>) -> Self {
        Self::from_kind(Kind::Array(value))
    }
}

impl From<ConfigMap> for ConfigValue {
    fn from(value: ConfigMap) -> Self {
        Self::from_kind(Kind::Map(value))
    }
}

impl<T: Into<Self>> From<Option<T>> for ConfigValue {
    fn from(value: Option<T>) -> Self {
        value.map_or_else(Self::null, Into::into)
    }
}

impl From<Kind> for ConfigValue {
    fn from(kind: Kind) -> Self {
        Self::from_kind(kind)
    }
}
