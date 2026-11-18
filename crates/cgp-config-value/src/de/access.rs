//! `SeqAccess`, `MapAccess`, `EnumAccess`, and the key deserializer.

use alloc::format;
use alloc::sync::Arc;

use serde::de::{self, DeserializeSeed, Visitor};

use super::{Ctx, DeserializeError, Path, ValueDeserializer};
use crate::{ConfigMap, ConfigValue};

pub(crate) struct SeqDe<'v, 'p> {
    iter: core::iter::Enumerate<core::slice::Iter<'v, ConfigValue>>,
    path: &'p Path<'p>,
    ctx: Option<&'p Ctx>,
}

impl<'v, 'p> SeqDe<'v, 'p> {
    pub(crate) fn new(
        elements: &'v [ConfigValue],
        path: &'p Path<'p>,
        ctx: Option<&'p Ctx>,
    ) -> Self {
        Self {
            iter: elements.iter().enumerate(),
            path,
            ctx,
        }
    }
}

impl<'de> de::SeqAccess<'de> for SeqDe<'_, '_> {
    type Error = DeserializeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        let Some((index, value)) = self.iter.next() else {
            return Ok(None);
        };
        let path = Path::Index {
            parent: self.path,
            index,
        };
        let de = ValueDeserializer {
            value,
            path: &path,
            ctx: self.ctx,
        };
        seed.deserialize(de).map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

pub(crate) struct MapDe<'v, 'p> {
    iter: indexmap::map::Iter<'v, Arc<str>, ConfigValue>,
    pending: Option<(&'v str, &'v ConfigValue)>,
    path: &'p Path<'p>,
    ctx: Option<&'p Ctx>,
    fields: Option<&'static [&'static str]>,
}

impl<'v, 'p> MapDe<'v, 'p> {
    pub(crate) fn new(
        map: &'v ConfigMap,
        path: &'p Path<'p>,
        ctx: Option<&'p Ctx>,
        fields: Option<&'static [&'static str]>,
    ) -> Self {
        Self {
            iter: map.raw_iter(),
            pending: None,
            path,
            ctx,
            fields,
        }
    }
}

impl<'de> de::MapAccess<'de> for MapDe<'_, '_> {
    type Error = DeserializeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        let Some((key, value)) = self.iter.next() else {
            return Ok(None);
        };
        let key: &str = key;
        self.pending = Some((key, value));
        let path = Path::Key {
            parent: self.path,
            key,
            fields: self.fields,
        };
        seed.deserialize(KeyDe { key }).map(Some).map_err(|error| {
            ValueDeserializer {
                value,
                path: &path,
                ctx: self.ctx,
            }
            .locate(error)
        })
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        let (key, value) = self
            .pending
            .take()
            .ok_or_else(|| <DeserializeError as de::Error>::custom("value requested before key"))?;
        let path = Path::Key {
            parent: self.path,
            key,
            fields: self.fields,
        };
        seed.deserialize(ValueDeserializer {
            value,
            path: &path,
            ctx: self.ctx,
        })
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

/// Deserializes a map key or a variant name. Keys are strings; numeric and bool targets
/// (`HashMap<u16, _>`) parse them.
pub(crate) struct KeyDe<'k> {
    pub(crate) key: &'k str,
}

macro_rules! parse_key {
    ($($method:ident => $visit:ident : $t:ty),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            let value = self.key.parse::<$t>().map_err(|_| {
                <DeserializeError as de::Error>::custom(format!(
                    concat!("expected a ", stringify!($t), " map key, found {:?}"),
                    self.key
                ))
            })?;
            visitor.$visit(value)
        }
    )*};
}

impl<'de> de::Deserializer<'de> for KeyDe<'_> {
    type Error = DeserializeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_str(self.key)
    }

    parse_key! {
        deserialize_bool => visit_bool: bool,
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
        deserialize_f32 => visit_f32: f32,
        deserialize_f64 => visit_f64: f64,
        deserialize_char => visit_char: char,
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_enum(de::value::StrDeserializer::<DeserializeError>::new(
            self.key,
        ))
    }

    serde::forward_to_deserialize_any! {
        str string bytes byte_buf unit unit_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }
}

/// `EnumAccess` for a variant written as a string (unit variant) or a one-key map.
pub(crate) struct EnumDe<'v, 'p> {
    variant: &'v str,
    value: Option<&'v ConfigValue>,
    path: &'p Path<'p>,
    ctx: Option<&'p Ctx>,
}

impl<'v, 'p> EnumDe<'v, 'p> {
    pub(crate) fn unit(variant: &'v Arc<str>, path: &'p Path<'p>, ctx: Option<&'p Ctx>) -> Self {
        Self {
            variant,
            value: None,
            path,
            ctx,
        }
    }

    pub(crate) fn with_value(
        variant: &'v str,
        value: &'v ConfigValue,
        path: &'p Path<'p>,
        ctx: Option<&'p Ctx>,
    ) -> Self {
        Self {
            variant,
            value: Some(value),
            path,
            ctx,
        }
    }
}

impl<'de, 'v, 'p> de::EnumAccess<'de> for EnumDe<'v, 'p> {
    type Error = DeserializeError;
    type Variant = VariantDe<'v, 'p>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Self::Error> {
        let tag = seed.deserialize(KeyDe { key: self.variant })?;
        Ok((
            tag,
            VariantDe {
                variant: self.variant,
                value: self.value,
                path: self.path,
                ctx: self.ctx,
            },
        ))
    }
}

pub(crate) struct VariantDe<'v, 'p> {
    variant: &'v str,
    value: Option<&'v ConfigValue>,
    path: &'p Path<'p>,
    ctx: Option<&'p Ctx>,
}

impl VariantDe<'_, '_> {
    fn missing_content(&self, what: &str) -> DeserializeError {
        <DeserializeError as de::Error>::custom(format!(
            "variant \"{}\" needs {what}; write it as a map with one key",
            self.variant
        ))
    }
}

impl<'de> de::VariantAccess<'de> for VariantDe<'_, '_> {
    type Error = DeserializeError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        match self.value {
            None => Ok(()),
            Some(value) if value.is_null() => Ok(()),
            Some(value) => Err(<DeserializeError as de::Error>::custom(format!(
                "variant \"{}\" takes no value, found {}",
                self.variant,
                value.kind_name()
            ))),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, Self::Error> {
        let value = self.value.ok_or_else(|| self.missing_content("a value"))?;
        let path = Path::Key {
            parent: self.path,
            key: self.variant,
            fields: None,
        };
        seed.deserialize(ValueDeserializer {
            value,
            path: &path,
            ctx: self.ctx,
        })
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let value = self
            .value
            .ok_or_else(|| self.missing_content("a sequence"))?;
        let path = Path::Key {
            parent: self.path,
            key: self.variant,
            fields: None,
        };
        de::Deserializer::deserialize_seq(
            ValueDeserializer {
                value,
                path: &path,
                ctx: self.ctx,
            },
            visitor,
        )
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let value = self.value.ok_or_else(|| self.missing_content("fields"))?;
        let path = Path::Key {
            parent: self.path,
            key: self.variant,
            fields: None,
        };
        de::Deserializer::deserialize_struct(
            ValueDeserializer {
                value,
                path: &path,
                ctx: self.ctx,
            },
            "",
            fields,
            visitor,
        )
    }
}
