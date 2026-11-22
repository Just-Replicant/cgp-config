//! A serde `Deserializer` over [`ConfigValue`] that keeps provenance.
//!
//! * **Errors carry the key path and the node's metadata.** A failing field becomes
//!   `ConfigError::Type { path: http.port, meta: <origin, span, overrides> }`; a missing
//!   field `ConfigError::Missing { path: http.port }`.
//! * **Strings are coerced by the target type.** Env and CLI layers store strings. When the
//!   target is a bool (`true` / `false`, ASCII case-insensitive, for values and map keys
//!   alike), an integer or float (`FromStr` of the target type), a `char`, or a sequence
//!   (JSON array text such as `[1, 2]`, never comma splitting), the string is parsed.
//!   Other targets read a string only as a string.
//! * **`deserialize_any` does not coerce.** Untagged enums, `#[serde(flatten)]`, and
//!   `serde_json::Value` targets ask serde for "any" value and see an env string such as
//!   `"8080"` as a string, not a number. This is deliberate: without a target type there is
//!   no safe way to guess (`"0123"`, `"1e3"`, `"true"` are valid strings). Give such fields
//!   a concrete type, or use a custom `Deserialize` that calls `deserialize_u16` & co.
//! * **Floats stay finite.** A finite input that does not fit the target float (`1e300`
//!   into `f32`, the string `"1e39"` into `f32`) is an error rather than infinity. Explicit
//!   `inf` / `nan` text or values pass through.
//! * **JSON array text obeys the load limits.** A sequence written as JSON text is parsed
//!   under the same `max_depth` and `max_keys` as the rest of the tree (counting from the
//!   string's own depth and the tree's existing entries) and fails with
//!   `ConfigError::LimitExceeded` before it allocates past them. [`from_value`] and
//!   [`from_value_with_unknown`] use [`LoadLimits::DEFAULT`]; [`from_value_with_limits`]
//!   takes the context's limits.
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
use core::cell::{Cell, RefCell};
use core::fmt;
use core::str::FromStr;

pub use cgp_config_core::UnknownKey;
use cgp_config_core::{ConfigError, KeyPath, LoadLimits, Meta, PathSegment, suggest};
use serde::de::{self, DeserializeOwned, Expected, IntoDeserializer, Unexpected, Visitor};

use self::access::{EnumDe, MapDe, SeqDe};
use crate::{ConfigValue, Kind};

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
/// Coercion needs a concrete target type. Targets that ask serde for *any* value
/// (untagged enums, `#[serde(flatten)]`, `serde_json::Value`) receive the env string
/// `"8080"` as a string, not a number:
///
/// ```
/// use cgp_config_value::{ConfigValue, from_value};
///
/// #[derive(serde::Deserialize, Debug, PartialEq)]
/// #[serde(untagged)]
/// enum Port { Number(u16), Named(String) }
///
/// let port: Port = from_value(&ConfigValue::from("8080"))?;
/// assert_eq!(port, Port::Named("8080".into()));
/// # Ok::<(), cgp_config_core::ConfigError>(())
/// ```
///
/// # Errors
///
/// `ConfigError::Type`, `Missing`, or `Unknown` (for `#[serde(deny_unknown_fields)]`
/// structs), with the failing path and provenance.
pub fn from_value<T: DeserializeOwned>(value: &ConfigValue) -> Result<T, ConfigError> {
    let ctx = Ctx::new(value, &LoadLimits::DEFAULT, false);
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
    from_value_with_limits(value, &LoadLimits::DEFAULT)
}

/// [`from_value_with_unknown`] under `limits`: sequences written as JSON text must stay
/// within `max_depth` and `max_keys`, counted together with the rest of `value`.
///
/// ```
/// use cgp_config_core::LoadLimits;
/// use cgp_config_value::{ConfigValue, from_value_with_limits};
///
/// #[derive(serde::Deserialize, Debug)]
/// struct App { ids: Vec<u32> }
///
/// let tree = ConfigValue::map([("ids", ConfigValue::from("[1, 2, 3, 4]"))]);
/// let limits = LoadLimits::DEFAULT.with_max_keys(3);
/// let error = from_value_with_limits::<App>(&tree, &limits).unwrap_err();
/// assert_eq!(error.code(), "config::limit_exceeded");
/// # Ok::<(), cgp_config_core::ConfigError>(())
/// ```
///
/// # Errors
///
/// As [`from_value`], and `ConfigError::LimitExceeded` when JSON array text exceeds a
/// limit.
pub fn from_value_with_limits<T: DeserializeOwned>(
    value: &ConfigValue,
    limits: &LoadLimits,
) -> Result<(T, Vec<UnknownKey>), ConfigError> {
    let ctx = Ctx::new(value, limits, true);
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

/// State shared by every node of one deserialization.
pub(crate) struct Ctx {
    unknown: Option<RefCell<Vec<UnknownKey>>>,
    limits: LoadLimits,
    /// Entries that trees expanded from JSON text may still add under `max_keys`.
    keys_left: Cell<usize>,
}

impl Ctx {
    fn new(root: &ConfigValue, limits: &LoadLimits, collect_unknown: bool) -> Self {
        Self {
            unknown: collect_unknown.then(|| RefCell::new(Vec::new())),
            limits: *limits,
            keys_left: Cell::new(limits.max_keys.saturating_sub(root.count_entries())),
        }
    }
}

/// `true` / `false` in any ASCII case. Values and map keys share this rule.
pub(crate) fn parse_bool(text: &str) -> Option<bool> {
    if text.eq_ignore_ascii_case("true") {
        Some(true)
    } else if text.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// A float target type.
pub(crate) trait Float: Copy + FromStr {
    fn is_finite(self) -> bool;
    fn from_f64(value: f64) -> Self;
}

impl Float for f32 {
    fn is_finite(self) -> bool {
        f32::is_finite(self)
    }

    #[allow(clippy::cast_possible_truncation)]
    fn from_f64(value: f64) -> Self {
        value as f32
    }
}

impl Float for f64 {
    fn is_finite(self) -> bool {
        f64::is_finite(self)
    }

    fn from_f64(value: f64) -> Self {
        value
    }
}

/// Whether `text` spells infinity or NaN on purpose (`inf`, `-Infinity`, `NaN`, ...).
fn is_explicit_non_finite(text: &str) -> bool {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    ["inf", "infinity", "nan"]
        .iter()
        .any(|word| unsigned.eq_ignore_ascii_case(word))
}

/// Parses a float, rejecting finite text that overflows the target (`"1e39"` as `f32`).
pub(crate) fn parse_float<T: Float>(text: &str) -> Option<T> {
    let value = text.parse::<T>().ok()?;
    (value.is_finite() || is_explicit_non_finite(text)).then_some(value)
}

/// Converts a number, rejecting a finite number that overflows the target.
fn number_to_float<T: Float>(number: &crate::Number) -> Option<T> {
    let wide = number.as_f64();
    let value = T::from_f64(wide);
    (value.is_finite() || !wide.is_finite()).then_some(value)
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
    /// Number of segments from the root (the node's depth in the tree).
    fn depth(self) -> usize {
        let mut depth = 0;
        let mut cursor = self;
        loop {
            match cursor {
                Path::Root => return depth,
                Path::Key { parent, .. } | Path::Index { parent, .. } => {
                    depth += 1;
                    cursor = *parent;
                }
            }
        }
    }

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
            Inner::Custom(message) => interpret_custom(message, KeyPath::root(), None),
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

fn interpret_custom(message: String, path: KeyPath, meta: Option<Meta>) -> ConfigError {
    cgp_config_core::parse_secret_session_error(&message)
        .unwrap_or_else(|| ConfigError::type_error(path, message, meta))
}

impl<'v, 'p> ValueDeserializer<'v, 'p> {
    /// Attaches the current path and metadata to an error that has none yet.
    pub(crate) fn locate(&self, error: DeserializeError) -> DeserializeError {
        let path = || self.path.to_key_path();
        let meta = || self.value.meta().clone();
        let located = match error.inner {
            Inner::Located(_) => return error,
            Inner::Custom(message) => interpret_custom(message, path(), Some(meta())),
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
            unknown.borrow_mut().push(UnknownKey::new(
                self.path.to_key_path(),
                suggest(key, fields.iter().copied()).map(ToOwned::to_owned),
                self.value.meta().clone(),
            ));
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
        // Without a context (`T::deserialize(&value)`), the defaults apply to this text alone.
        let standalone;
        let (limits, keys_left) = match self.ctx {
            Some(ctx) => (&ctx.limits, &ctx.keys_left),
            None => {
                standalone = Cell::new(LoadLimits::DEFAULT.max_keys);
                (&LoadLimits::DEFAULT, &standalone)
            }
        };
        let mut parsed =
            crate::serde_impls::from_json_limited(text, limits, self.path.depth(), keys_left)
                .map_err(|error| match error {
                    crate::serde_impls::LimitedJsonError::Limit(error) => {
                        DeserializeError::located(error)
                    }
                    crate::serde_impls::LimitedJsonError::Syntax(message) => self.error(format!(
                        "expected a sequence, found invalid JSON array text: {message}"
                    )),
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
            let value: $t = match self.value.kind() {
                Kind::Number(number) => number_to_float::<$t>(number).ok_or_else(|| {
                    self.error(format!(
                        concat!("expected ", stringify!($t), ", found {} (out of range)"),
                        number
                    ))
                })?,
                Kind::String(text) => parse_float::<$t>(text).ok_or_else(|| {
                    self.error(format!(
                        concat!("expected ", stringify!($t), ", found string {:?}"),
                        text
                    ))
                })?,
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
            Kind::String(text) => parse_bool(text).ok_or_else(|| {
                self.error(format!(
                    "expected bool (true or false), found string {text:?}"
                ))
            })?,
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
