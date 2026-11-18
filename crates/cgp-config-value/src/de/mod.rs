//! A serde `Deserializer` over [`ConfigValue`] that keeps provenance.
//!
//! * **Errors carry the key path and the node's metadata.** A failing field becomes
//!   `ConfigError::Type { path: http.port, meta: <origin, span, overrides> }`; a missing
//!   field `ConfigError::Missing { path: http.port }`.
//! * **Strings are coerced by the target type.** Env and CLI layers store strings. When the
//!   target is a bool (`true` / `false`, ASCII case-insensitive), an integer or float
//!   (`FromStr` of the target type), a `char`, or a sequence (JSON array text such as
//!   `[1, 2]`, never comma splitting), the string is parsed. Other targets read a string
//!   only as a string. `deserialize_any` (untagged enums, `#[serde(flatten)]`) sees the
//!   raw string.
//! * **Secret paths never echo input.** On a node whose `Meta::secret` is set, every
//!   message (including serde's `invalid value: string "..."`) is replaced by a fixed text.
//! * **Unknown keys are collected, not lost.** A key serde ignores while deserializing a
//!   struct is recorded as an [`UnknownKey`] with a "did you mean" suggestion from the
//!   struct's declared fields. The `SerdeWithProvenance` policy decides whether that warns
//!   or fails. Keys under `#[serde(flatten)]` are buffered by serde and not reported.
//! * **No JSON round-trip and no `Serialize` bound** on the config type. Strings are
//!   handed to visitors by reference (`visit_str`), so targets are `DeserializeOwned`.

mod access;

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;
use core::str::FromStr;

use cgp_config_core::{ConfigError, KeyPath, Meta, PathSegment, suggest};
use serde::de::{self, DeserializeOwned, Expected, IntoDeserializer, Unexpected, Visitor};

use self::access::{EnumDe, MapDe, SeqDe};
use crate::{ConfigValue, Kind};

/// A key that the target struct does not declare.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct UnknownKey {
    /// The key's path.
    pub path: KeyPath,
    /// The closest declared field name, when the key looks like a typo of it.
    pub suggestion: Option<String>,
    /// The key's provenance.
    pub meta: Meta,
}

/// Deserializes `T` from `value`.
///
/// ```
/// use cgp_config_value::{ConfigValue, from_value};
///
/// #[derive(serde::Deserialize, Debug, PartialEq)]
/// struct Http { port: u16, tls: bool, hosts: Vec<String> }
///
/// // As an env layer would store it: every leaf is a string.
/// let tree = ConfigValue::map([
///     ("port", ConfigValue::from("8080")),
///     ("tls", ConfigValue::from("TRUE")),
///     ("hosts", ConfigValue::from(r#"["a", "b"]"#)),
/// ]);
/// let http: Http = from_value(&tree)?;
/// assert_eq!(http, Http { port: 8080, tls: true, hosts: vec!["a".into(), "b".into()] });
/// # Ok::<(), cgp_config_core::ConfigError>(())
/// ```
///
/// # Errors
///
/// `ConfigError::Type`, `Missing`, or `Unknown` (for `#[serde(deny_unknown_fields)]`
/// structs), with the failing path and provenance.
pub fn from_value<T: DeserializeOwned>(value: &ConfigValue) -> Result<T, ConfigError> {
    let ctx = Ctx::default();
    deserialize_root(value, &ctx)
}

/// Deserializes `T` from `value` and returns the keys `T` does not declare.
///
/// # Errors
///
/// As [`from_value`].
pub fn from_value_with_unknown<T: DeserializeOwned>(
    value: &ConfigValue,
) -> Result<(T, Vec<UnknownKey>), ConfigError> {
    let ctx = Ctx {
        unknown: Some(RefCell::new(Vec::new())),
    };
    let result = deserialize_root(value, &ctx)?;
    let unknown = ctx.unknown.map(RefCell::into_inner).unwrap_or_default();
    Ok((result, unknown))
}

fn deserialize_root<T: DeserializeOwned>(value: &ConfigValue, ctx: &Ctx) -> Result<T, ConfigError> {
    let de = ValueDeserializer {
        value,
        path: &Path::Root,
        ctx: Some(ctx),
    };
    T::deserialize(de).map_err(|error| {
        ValueDeserializer::root(value)
            .locate(error)
            .into_config_error()
    })
}

#[derive(Default)]
pub(crate) struct Ctx {
    unknown: Option<RefCell<Vec<UnknownKey>>>,
}

/// The current position as a linked list on the stack; materialized only on error.
#[derive(Clone, Copy)]
pub(crate) enum Path<'p> {
    Root,
    Key {
        parent: &'p Path<'p>,
        key: &'p str,
        /// Fields of the struct that holds this key, for unknown-key suggestions.
        fields: Option<&'static [&'static str]>,
    },
    Index {
        parent: &'p Path<'p>,
        index: usize,
    },
}

impl Path<'_> {
    fn to_key_path(self) -> KeyPath {
        let mut segments = Vec::new();
        let mut cursor = self;
        loop {
            match cursor {
                Path::Root => break,
                Path::Key { parent, key, .. } => {
                    segments.push(PathSegment::from(key));
                    cursor = *parent;
                }
                Path::Index { parent, index } => {
                    segments.push(PathSegment::Index(index));
                    cursor = *parent;
                }
            }
        }
        segments.reverse();
        KeyPath::from_segments(segments)
    }
}

/// The error type of the serde layer. Converts into [`ConfigError`].
#[derive(Debug, Clone, PartialEq)]
pub struct DeserializeError {
    inner: Inner,
}

#[derive(Debug, Clone, PartialEq)]
enum Inner {
    Custom(String),
    Missing(&'static str),
    UnknownField {
        field: String,
        expected: &'static [&'static str],
    },
    Located(ConfigError),
}

impl DeserializeError {
    fn located(error: ConfigError) -> Self {
        Self {
            inner: Inner::Located(error),
        }
    }

    /// The error as a [`ConfigError`]. An error that never passed through a node (only
    /// possible for errors built outside this deserializer) becomes a type error at the
    /// root.
    #[must_use]
    pub fn into_config_error(self) -> ConfigError {
        match self.inner {
            Inner::Located(error) => error,
            Inner::Custom(message) => ConfigError::type_error(KeyPath::root(), message, None),
            Inner::Missing(field) => {
                ConfigError::missing(KeyPath::root().join(field), String::new())
            }
            Inner::UnknownField { field, expected } => ConfigError::unknown(
                KeyPath::root().join(field.as_str()),
                suggest(&field, expected.iter().copied()).map(ToOwned::to_owned),
                Meta::default(),
            ),
        }
    }
}

impl From<DeserializeError> for ConfigError {
    fn from(error: DeserializeError) -> Self {
        error.into_config_error()
    }
}

impl fmt::Display for DeserializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            Inner::Custom(message) => f.write_str(message),
            Inner::Missing(field) => write!(f, "missing field `{field}`"),
            Inner::UnknownField { field, .. } => write!(f, "unknown field `{field}`"),
            Inner::Located(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl core::error::Error for DeserializeError {}

fn one_of(names: &[&str]) -> String {
    match names {
        [] => "no variants".to_owned(),
        [only] => format!("{only:?}"),
        names => {
            let quoted: Vec<String> = names.iter().map(|name| format!("{name:?}")).collect();
            format!("one of {}", quoted.join(", "))
        }
    }
}

impl de::Error for DeserializeError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self {
            inner: Inner::Custom(message.to_string()),
        }
    }

    fn missing_field(field: &'static str) -> Self {
        Self {
            inner: Inner::Missing(field),
        }
    }

    fn unknown_field(field: &str, expected: &'static [&'static str]) -> Self {
        Self {
            inner: Inner::UnknownField {
                field: field.to_owned(),
                expected,
            },
        }
    }

    fn unknown_variant(variant: &str, expected: &'static [&'static str]) -> Self {
        let mut message = format!(
            "unknown variant \"{variant}\", expected {}",
            one_of(expected)
        );
        if let Some(close) = suggest(variant, expected.iter().copied()) {
            message.push_str(&format!("; did you mean \"{close}\"?"));
        }
        Self::custom(message)
    }
}

/// The deserializer for one node.
pub(crate) struct ValueDeserializer<'v, 'p> {
    pub(crate) value: &'v ConfigValue,
    pub(crate) path: &'p Path<'p>,
    pub(crate) ctx: Option<&'p Ctx>,
}

impl<'v> ValueDeserializer<'v, 'static> {
    fn root(value: &'v ConfigValue) -> Self {
        Self {
            value,
            path: &Path::Root,
            ctx: None,
        }
    }
}

fn unexpected(value: &ConfigValue) -> Unexpected<'_> {
    match value.kind() {
        Kind::Null => Unexpected::Unit,
        Kind::Bool(b) => Unexpected::Bool(*b),
        Kind::Number(n) => match (n.as_u64(), n.as_i64()) {
            (Some(u), _) => Unexpected::Unsigned(u),
            (None, Some(i)) => Unexpected::Signed(i),
            _ => Unexpected::Float(n.as_f64()),
        },
        Kind::String(s) => Unexpected::Str(s),
        Kind::Array(_) => Unexpected::Seq,
        Kind::Map(_) => Unexpected::Map,
    }
}

impl<'v, 'p> ValueDeserializer<'v, 'p> {
    /// Attaches the current path and metadata to an error that has none yet.
    pub(crate) fn locate(&self, error: DeserializeError) -> DeserializeError {
        let path = || self.path.to_key_path();
        let meta = || self.value.meta().clone();
        let located = match error.inner {
            Inner::Located(_) => return error,
            Inner::Custom(message) => ConfigError::type_error(path(), message, Some(meta())),
            Inner::Missing(field) => ConfigError::missing(path().join(field), String::new()),
            Inner::UnknownField { field, expected } => {
                // Raised either by the key itself (serde's field visitor; `self` is then the
                // key's node) or by the struct after the fact (`self` is the map).
                let at_key = matches!(self.path, Path::Key { key, .. } if *key == field);
                let (key_path, key_meta) = if at_key {
                    (path(), meta())
                } else {
                    let key_meta = self
                        .value
                        .get(&field)
                        .map_or_else(meta, |v| v.meta().clone());
                    (path().join(field.as_str()), key_meta)
                };
                ConfigError::unknown(
                    key_path,
                    suggest(&field, expected.iter().copied()).map(ToOwned::to_owned),
                    key_meta,
                )
            }
        };
        DeserializeError::located(located)
    }

    fn error(&self, message: String) -> DeserializeError {
        self.locate(de::Error::custom(message))
    }

    fn invalid_type(&self, expected: &dyn Expected) -> DeserializeError {
        self.locate(de::Error::invalid_type(unexpected(self.value), expected))
    }

    fn parse_str<T: FromStr>(&self, text: &str, target: &str) -> Result<T, DeserializeError> {
        text.parse::<T>()
            .map_err(|_| self.error(format!("expected {target}, found string {text:?}")))
    }

    fn child<'q>(&self, value: &'q ConfigValue, path: &'q Path<'q>) -> ValueDeserializer<'q, 'q>
    where
        'p: 'q,
    {
        ValueDeserializer {
            value,
            path,
            ctx: self.ctx,
        }
    }

    fn record_unknown(&self) {
        let (
            Some(ctx),
            Path::Key {
                key,
                fields: Some(fields),
                ..
            },
        ) = (self.ctx, self.path)
        else {
            return;
        };
        if let Some(unknown) = &ctx.unknown {
            unknown.borrow_mut().push(UnknownKey {
                path: self.path.to_key_path(),
                suggestion: suggest(key, fields.iter().copied()).map(ToOwned::to_owned),
                meta: self.value.meta().clone(),
            });
        }
    }

    fn deserialize_json_seq<'de, V: Visitor<'de>>(
        self,
        text: &str,
        visitor: V,
    ) -> Result<V::Value, DeserializeError> {
        if !text.trim_start().starts_with('[') {
            return Err(self.error(format!(
                "expected a sequence written as JSON array text such as [1, 2], found string {text:?}"
            )));
        }
        let mut parsed: ConfigValue = serde_json::from_str(text).map_err(|error| {
            self.error(format!(
                "expected a sequence, found invalid JSON array text: {error}"
            ))
        })?;
        let source = self.value.meta();
        parsed.stamp_origin(&source.origin);
        if source.secret {
            mark_secret(&mut parsed);
        }
        let de = self.child(&parsed, self.path);
        de.deserialize_seq_inner(visitor)
    }

    fn deserialize_seq_inner<'de, V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, DeserializeError> {
        match self.value.kind() {
            Kind::Array(elements) => {
                let access = SeqDe::new(elements, self.path, self.ctx);
                visitor.visit_seq(access).map_err(|e| self.locate(e))
            }
            Kind::String(text) => {
                let text = text.clone();
                self.deserialize_json_seq(&text, visitor)
            }
            _ => Err(self.invalid_type(&visitor)),
        }
    }
}

fn mark_secret(value: &mut ConfigValue) {
    value.meta_mut().secret = true;
    match value.kind_mut() {
        Kind::Array(elements) => elements.iter_mut().for_each(mark_secret),
        Kind::Map(map) => map.values_mut().for_each(mark_secret),
        _ => {}
    }
}

macro_rules! deserialize_integer {
    ($($method:ident => $visit:ident : $t:ty),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            let value: $t = match self.value.kind() {
                Kind::Number(number) => match number.as_i128().and_then(|v| <$t>::try_from(v).ok()) {
                    Some(value) => value,
                    None if number.is_float() => {
                        return Err(self.error(format!(
                            concat!("expected ", stringify!($t), ", found float {}"),
                            number
                        )));
                    }
                    None => {
                        return Err(self.error(format!(
                            concat!("expected ", stringify!($t), ", found {} (out of range)"),
                            number
                        )));
                    }
                },
                Kind::String(text) => self.parse_str::<$t>(text, stringify!($t))?,
                _ => return Err(self.invalid_type(&visitor)),
            };
            visitor.$visit(value).map_err(|e| self.locate(e))
        }
    )*};
}

macro_rules! deserialize_float {
    ($($method:ident => $visit:ident : $t:ty),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            #[allow(clippy::cast_possible_truncation)]
            let value: $t = match self.value.kind() {
                Kind::Number(number) => number.as_f64() as $t,
                Kind::String(text) => self.parse_str::<$t>(text, stringify!($t))?,
                _ => return Err(self.invalid_type(&visitor)),
            };
            visitor.$visit(value).map_err(|e| self.locate(e))
        }
    )*};
}

impl<'de> de::Deserializer<'de> for ValueDeserializer<'_, '_> {
    type Error = DeserializeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let result = match self.value.kind() {
            Kind::Null => visitor.visit_unit(),
            Kind::Bool(value) => visitor.visit_bool(*value),
            Kind::Number(number) => match (number.as_u64(), number.as_i64()) {
                (Some(value), _) => visitor.visit_u64(value),
                (None, Some(value)) => visitor.visit_i64(value),
                _ => visitor.visit_f64(number.as_f64()),
            },
            Kind::String(text) => visitor.visit_str(text),
            Kind::Array(elements) => visitor.visit_seq(SeqDe::new(elements, self.path, self.ctx)),
            Kind::Map(map) => visitor.visit_map(MapDe::new(map, self.path, self.ctx, None)),
        };
        result.map_err(|e| self.locate(e))
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let value = match self.value.kind() {
            Kind::Bool(value) => *value,
            Kind::String(text) if text.eq_ignore_ascii_case("true") => true,
            Kind::String(text) if text.eq_ignore_ascii_case("false") => false,
            Kind::String(text) => {
                return Err(self.error(format!(
                    "expected bool (true or false), found string {text:?}"
                )));
            }
            _ => return Err(self.invalid_type(&visitor)),
        };
        visitor.visit_bool(value).map_err(|e| self.locate(e))
    }

    deserialize_integer! {
        deserialize_i8 => visit_i8: i8,
        deserialize_i16 => visit_i16: i16,
        deserialize_i32 => visit_i32: i32,
        deserialize_i64 => visit_i64: i64,
        deserialize_i128 => visit_i128: i128,
        deserialize_u8 => visit_u8: u8,
        deserialize_u16 => visit_u16: u16,
        deserialize_u32 => visit_u32: u32,
        deserialize_u64 => visit_u64: u64,
        deserialize_u128 => visit_u128: u128,
    }

    deserialize_float! {
        deserialize_f32 => visit_f32: f32,
        deserialize_f64 => visit_f64: f64,
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.value.kind() {
            Kind::String(text) => {
                let mut chars = text.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => visitor.visit_char(c).map_err(|e| self.locate(e)),
                    _ => Err(self.error(format!(
                        "expected a single character, found string {text:?}"
                    ))),
                }
            }
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.value.kind() {
            Kind::String(text) => visitor.visit_str(text).map_err(|e| self.locate(e)),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.value.kind() {
            Kind::String(text) => visitor
                .visit_bytes(text.as_bytes())
                .map_err(|e| self.locate(e)),
            Kind::Array(elements) => visitor
                .visit_seq(SeqDe::new(elements, self.path, self.ctx))
                .map_err(|e| self.locate(e)),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if self.value.is_null() {
            visitor.visit_none().map_err(|e| self.locate(e))
        } else {
            let path = self.path;
            let value = self.value;
            let ctx = self.ctx;
            visitor
                .visit_some(ValueDeserializer { value, path, ctx })
                .map_err(|e| ValueDeserializer { value, path, ctx }.locate(e))
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if self.value.is_null() {
            visitor.visit_unit().map_err(|e| self.locate(e))
        } else {
            Err(self.invalid_type(&visitor))
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let (value, path, ctx) = (self.value, self.path, self.ctx);
        visitor
            .visit_newtype_struct(ValueDeserializer { value, path, ctx })
            .map_err(|e| ValueDeserializer { value, path, ctx }.locate(e))
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_seq_inner(visitor)
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_seq_inner(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_seq_inner(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.value.kind() {
            Kind::Map(map) => visitor
                .visit_map(MapDe::new(map, self.path, self.ctx, None))
                .map_err(|e| self.locate(e)),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        match self.value.kind() {
            Kind::Map(map) => visitor
                .visit_map(MapDe::new(map, self.path, self.ctx, Some(fields)))
                .map_err(|e| self.locate(e)),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let access = match self.value.kind() {
            Kind::String(text) => EnumDe::unit(text, self.path, self.ctx),
            Kind::Map(map) if map.len() == 1 => {
                let (key, value) = map.iter().next().expect("one entry");
                EnumDe::with_value(key, value, self.path, self.ctx)
            }
            _ => {
                return Err(self.error(format!(
                    "expected an enum variant (a string or a map with one key), found {}",
                    self.value.kind_name()
                )));
            }
        };
        visitor.visit_enum(access).map_err(|e| self.locate(e))
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.record_unknown();
        visitor.visit_unit().map_err(|e| self.locate(e))
    }
}

macro_rules! forward_to_root {
    ($($method:ident ( $($arg:ident : $ty:ty),* )),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, $($arg: $ty,)* visitor: V) -> Result<V::Value, Self::Error> {
            ValueDeserializer::root(self).$method($($arg,)* visitor)
        }
    )*};
}

/// `&ConfigValue` is itself a serde `Deserializer` (without unknown-key collection), so
/// `T::deserialize(&value)` works with any serde-based API.
impl<'de> de::Deserializer<'de> for &ConfigValue {
    type Error = DeserializeError;

    forward_to_root! {
        deserialize_any(), deserialize_bool(), deserialize_i8(), deserialize_i16(),
        deserialize_i32(), deserialize_i64(), deserialize_i128(), deserialize_u8(),
        deserialize_u16(), deserialize_u32(), deserialize_u64(), deserialize_u128(),
        deserialize_f32(), deserialize_f64(), deserialize_char(), deserialize_str(),
        deserialize_string(), deserialize_bytes(), deserialize_byte_buf(), deserialize_option(),
        deserialize_unit(), deserialize_unit_struct(name: &'static str),
        deserialize_newtype_struct(name: &'static str), deserialize_seq(),
        deserialize_tuple(len: usize), deserialize_tuple_struct(name: &'static str, len: usize),
        deserialize_map(),
        deserialize_struct(name: &'static str, fields: &'static [&'static str]),
        deserialize_enum(name: &'static str, variants: &'static [&'static str]),
        deserialize_identifier(), deserialize_ignored_any(),
    }
}

impl<'de, 'v> IntoDeserializer<'de, DeserializeError> for &'v ConfigValue {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}
