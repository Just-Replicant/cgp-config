//! `Serialize` and `Deserialize` for [`ConfigValue`] itself.
//!
//! Serializing writes the data and drops metadata (secret *references* are data and are
//! written as stored; use [`ConfigValue::redacted`] for output a person reads).
//! Deserializing builds a tree with unknown origin from any serde format, which is how
//! JSON text from env (`APP__HOSTS='["a","b"]'`) becomes a sequence.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::Cell;
use core::fmt;

use cgp_config_core::{ConfigError, LoadLimits};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{ConfigMap, ConfigValue, Kind, Number};

impl Serialize for ConfigValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.kind() {
            Kind::Null => serializer.serialize_unit(),
            Kind::Bool(value) => serializer.serialize_bool(*value),
            Kind::Number(number) => number.serialize(serializer),
            Kind::String(text) => serializer.serialize_str(text),
            Kind::Array(elements) => {
                let mut seq = serializer.serialize_seq(Some(elements.len()))?;
                for element in elements {
                    seq.serialize_element(element)?;
                }
                seq.end()
            }
            Kind::Map(map) => {
                let mut out = serializer.serialize_map(Some(map.len()))?;
                for (key, value) in map.iter() {
                    out.serialize_entry(key, value)?;
                }
                out.end()
            }
        }
    }
}

impl Serialize for Number {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(value) = self.as_u64() {
            serializer.serialize_u64(value)
        } else if let Some(value) = self.as_i64() {
            serializer.serialize_i64(value)
        } else {
            serializer.serialize_f64(self.as_f64())
        }
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = ConfigValue;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any configuration value")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<ConfigValue, E> {
        Ok(ConfigValue::from(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<ConfigValue, E> {
        Ok(ConfigValue::from(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<ConfigValue, E> {
        Ok(ConfigValue::from(value))
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<ConfigValue, E> {
        if let Ok(value) = u64::try_from(value) {
            Ok(ConfigValue::from(value))
        } else if let Ok(value) = i64::try_from(value) {
            Ok(ConfigValue::from(value))
        } else {
            Err(E::custom("integer out of the 64-bit range"))
        }
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<ConfigValue, E> {
        u64::try_from(value)
            .map(ConfigValue::from)
            .map_err(|_| E::custom("integer out of the 64-bit range"))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<ConfigValue, E> {
        Ok(ConfigValue::from(value))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<ConfigValue, E> {
        Ok(ConfigValue::from(value))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<ConfigValue, E> {
        Ok(ConfigValue::from(value))
    }

    fn visit_unit<E: de::Error>(self) -> Result<ConfigValue, E> {
        Ok(ConfigValue::null())
    }

    fn visit_none<E: de::Error>(self) -> Result<ConfigValue, E> {
        Ok(ConfigValue::null())
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<ConfigValue, D::Error> {
        ConfigValue::deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ConfigValue, A::Error> {
        let mut elements = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1024));
        while let Some(element) = seq.next_element()? {
            elements.push(element);
        }
        Ok(ConfigValue::from(elements))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<ConfigValue, A::Error> {
        let mut map = ConfigMap::with_capacity(access.size_hint().unwrap_or(0).min(1024));
        while let Some((key, value)) = access.next_entry::<String, ConfigValue>()? {
            if map.contains_key(&key) {
                return Err(de::Error::custom(alloc::format!("duplicate key `{key}`")));
            }
            map.insert(Arc::<str>::from(key), value);
        }
        Ok(ConfigValue::from(Kind::Map(map)))
    }
}

impl<'de> Deserialize<'de> for ConfigValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

/// Why [`from_json_limited`] failed.
pub(crate) enum LimitedJsonError {
    /// A [`LoadLimits`] bound was hit; the parse stopped there.
    Limit(ConfigError),
    /// The text is not valid JSON (serde_json's message, which never quotes the input).
    Syntax(String),
}

/// The limits of one bounded parse. `keys_left` is shared with the caller, so several JSON
/// strings in one tree draw from the same `max_keys` budget.
struct Budget<'a> {
    limits: &'a LoadLimits,
    keys_left: &'a Cell<usize>,
    exceeded: Cell<Option<ConfigError>>,
}

impl Budget<'_> {
    /// Admits one more entry at `depth`, or records the exceeded limit.
    fn admit<E: de::Error>(&self, depth: usize) -> Result<(), E> {
        if depth > self.limits.max_depth {
            return Err(self.fail("max_depth", self.limits.max_depth));
        }
        let left = self.keys_left.get();
        if left == 0 {
            return Err(self.fail("max_keys", self.limits.max_keys));
        }
        self.keys_left.set(left - 1);
        Ok(())
    }

    fn fail<E: de::Error>(&self, what: &'static str, limit: usize) -> E {
        self.exceeded
            .set(Some(ConfigError::limit_exceeded(what, limit)));
        E::custom(alloc::format!("limit exceeded: {what} ({limit})"))
    }

    /// Capacity to reserve for a container: the hint, but never past the remaining budget.
    fn capacity(&self, hint: Option<usize>) -> usize {
        hint.unwrap_or(0).min(self.keys_left.get()).min(1024)
    }
}

/// Parses JSON `text` into a tree whose root sits at `depth`, admitting every element and
/// entry against `limits` *while* parsing, so neither nesting nor width can allocate past
/// `max_depth` / `max_keys` (the tree is never built first and checked afterwards).
pub(crate) fn from_json_limited(
    text: &str,
    limits: &LoadLimits,
    depth: usize,
    keys_left: &Cell<usize>,
) -> Result<ConfigValue, LimitedJsonError> {
    let budget = Budget {
        limits,
        keys_left,
        exceeded: Cell::new(None),
    };
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let parsed = LimitedSeed {
        budget: &budget,
        depth,
    }
    .deserialize(&mut deserializer)
    .and_then(|value| deserializer.end().map(|()| value));
    parsed.map_err(|error| match budget.exceeded.take() {
        Some(limit) => LimitedJsonError::Limit(limit),
        None => LimitedJsonError::Syntax(alloc::string::ToString::to_string(&error)),
    })
}

#[derive(Clone, Copy)]
struct LimitedSeed<'b, 'a> {
    budget: &'b Budget<'a>,
    /// Depth of the value this seed produces.
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for LimitedSeed<'_, '_> {
    type Value = ConfigValue;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<ConfigValue, D::Error> {
        deserializer.deserialize_any(self)
    }
}

/// An array element: admitted against the budget when the parser has found one, before
/// the element itself is parsed.
struct ElementSeed<'b, 'a>(LimitedSeed<'b, 'a>);

impl<'de> DeserializeSeed<'de> for ElementSeed<'_, '_> {
    type Value = ConfigValue;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<ConfigValue, D::Error> {
        self.0.budget.admit::<D::Error>(self.0.depth)?;
        self.0.deserialize(deserializer)
    }
}

macro_rules! forward_scalars {
    ($($method:ident($($arg:ident: $ty:ty)?)),* $(,)?) => {$(
        fn $method<E: de::Error>(self, $($arg: $ty)?) -> Result<ConfigValue, E> {
            ValueVisitor.$method($($arg)?)
        }
    )*};
}

impl<'de> Visitor<'de> for LimitedSeed<'_, '_> {
    type Value = ConfigValue;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        ValueVisitor.expecting(f)
    }

    forward_scalars! {
        visit_bool(value: bool),
        visit_i64(value: i64),
        visit_u64(value: u64),
        visit_i128(value: i128),
        visit_u128(value: u128),
        visit_f64(value: f64),
        visit_str(value: &str),
        visit_string(value: String),
        visit_unit(),
        visit_none(),
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<ConfigValue, D::Error> {
        self.deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ConfigValue, A::Error> {
        let child = LimitedSeed {
            depth: self.depth + 1,
            ..self
        };
        let mut elements = Vec::with_capacity(self.budget.capacity(seq.size_hint()));
        while let Some(element) = seq.next_element_seed(ElementSeed(child))? {
            elements.push(element);
        }
        Ok(ConfigValue::from(elements))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<ConfigValue, A::Error> {
        let child = LimitedSeed {
            depth: self.depth + 1,
            ..self
        };
        let mut map = ConfigMap::with_capacity(self.budget.capacity(access.size_hint()));
        while let Some(key) = access.next_key::<String>()? {
            self.budget.admit::<A::Error>(child.depth)?;
            if map.contains_key(&key) {
                return Err(de::Error::custom(alloc::format!("duplicate key `{key}`")));
            }
            let value = access.next_value_seed(child)?;
            map.insert(Arc::<str>::from(key), value);
        }
        Ok(ConfigValue::from(Kind::Map(map)))
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use crate::ConfigValue;

    #[test]
    fn json_round_trip() {
        let text = r#"{"a":[1,-2,1.5,true,null,"s"],"b":{"c":18446744073709551615}}"#;
        let value: ConfigValue = serde_json::from_str(text).unwrap();
        assert_eq!(
            value.get_path(&"a[1]".into()).and_then(ConfigValue::as_i64),
            Some(-2)
        );
        assert_eq!(serde_json::to_string(&value).unwrap(), text);
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let error = serde_json::from_str::<ConfigValue>(r#"{"a":1,"a":2}"#).unwrap_err();
        assert!(error.to_string().contains("duplicate key `a`"));
    }

    struct Expecting;

    impl core::fmt::Display for Expecting {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            serde::de::Visitor::expecting(&super::ValueVisitor, f)
        }
    }

    #[test]
    fn wide_integers() {
        use serde::de::Visitor;
        use serde::de::value::Error;
        let visitor = super::ValueVisitor;
        assert_eq!(visitor.visit_i128::<Error>(-5).unwrap().as_i64(), Some(-5));
        assert_eq!(
            super::ValueVisitor.visit_i128::<Error>(5).unwrap().as_u64(),
            Some(5)
        );
        assert!(super::ValueVisitor.visit_i128::<Error>(i128::MIN).is_err());
        assert_eq!(
            super::ValueVisitor.visit_u128::<Error>(5).unwrap().as_u64(),
            Some(5)
        );
        assert!(super::ValueVisitor.visit_u128::<Error>(u128::MAX).is_err());
        assert!(
            super::ValueVisitor
                .visit_string::<Error>("x".into())
                .unwrap()
                .as_str()
                == Some("x")
        );
        assert!(super::ValueVisitor.visit_none::<Error>().unwrap().is_null());
        let mut expecting = alloc::string::String::new();
        core::fmt::Write::write_fmt(&mut expecting, format_args!("{Expecting}")).unwrap();
        assert_eq!(expecting, "any configuration value");
    }
}
