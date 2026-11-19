//! CGP providers of `CanParseFormat<Fmt>` and `CanRenderFormat<Fmt>`.

// With no format feature only the empty table remains.
#![cfg_attr(not(any(feature = "toml", feature = "json")), allow(unused_imports))]

use alloc::string::String;

use cgp_config_core::{
    ConfigError, FormatParser, FormatParserComponent, FormatRenderer, FormatRendererComponent,
    HasConfigValueType, HasLoadLimits, Origin,
};
use cgp_config_value::ConfigValue;
use cgp_fork::prelude::*;

#[cfg(feature = "json")]
use crate::Json;
#[cfg(feature = "toml")]
use crate::Toml;

/// Parses and renders TOML (`CanParseFormat<Toml>`, `CanRenderFormat<Toml>`).
#[cfg(feature = "toml")]
pub struct TomlFormat;

#[cfg(feature = "toml")]
#[cgp_impl(TomlFormat)]
impl FormatParser<Toml>
where
    Self:
        HasConfigValueType<ConfigValue = ConfigValue> + CanRaiseError<ConfigError> + HasLoadLimits,
{
    fn parse_format(
        &self,
        _format: PhantomData<Toml>,
        origin: &Origin,
        text: &str,
    ) -> Result<Self::ConfigValue, Self::Error> {
        crate::parse_toml(origin, text, &self.load_limits()).map_err(Self::raise_error)
    }
}

#[cfg(feature = "toml")]
#[cgp_impl(TomlFormat)]
impl FormatRenderer<Toml>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue> + CanRaiseError<ConfigError>,
{
    fn render_format(
        &self,
        _format: PhantomData<Toml>,
        value: &Self::ConfigValue,
    ) -> Result<String, Self::Error> {
        crate::render_toml(value).map_err(Self::raise_error)
    }
}

/// Parses and renders JSON (`CanParseFormat<Json>`, `CanRenderFormat<Json>`).
#[cfg(feature = "json")]
pub struct JsonFormat;

#[cfg(feature = "json")]
#[cgp_impl(JsonFormat)]
impl FormatParser<Json>
where
    Self:
        HasConfigValueType<ConfigValue = ConfigValue> + CanRaiseError<ConfigError> + HasLoadLimits,
{
    fn parse_format(
        &self,
        _format: PhantomData<Json>,
        origin: &Origin,
        text: &str,
    ) -> Result<Self::ConfigValue, Self::Error> {
        crate::parse_json(origin, text, &self.load_limits()).map_err(Self::raise_error)
    }
}

#[cfg(feature = "json")]
#[cgp_impl(JsonFormat)]
impl FormatRenderer<Json>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue> + CanRaiseError<ConfigError>,
{
    fn render_format(
        &self,
        _format: PhantomData<Json>,
        value: &Self::ConfigValue,
    ) -> Result<String, Self::Error> {
        crate::kind::render_json(value).map_err(Self::raise_error)
    }
}

/// The per-format table of every format compiled into this build:
/// `Toml → TomlFormat` (feature `toml`), `Json → JsonFormat` (feature `json`). Wire it as
/// `FormatParserComponent: UseDelegate<EnabledFormats>` and
/// `FormatRendererComponent: UseDelegate<EnabledFormats>`.
pub struct EnabledFormats;

#[cfg(feature = "toml")]
delegate_components! {
    #[unchecked]
    EnabledFormats {
        Toml: TomlFormat,
    }
}

#[cfg(feature = "json")]
delegate_components! {
    #[unchecked]
    EnabledFormats {
        Json: JsonFormat,
    }
}
