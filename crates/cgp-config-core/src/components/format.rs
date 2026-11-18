use alloc::string::String;

use cgp_fork::prelude::*;

use crate::{HasConfigValueType, Origin};

/// Parses source text in the format identified by the marker type `Format`.
///
/// Every parser rejects duplicate keys, enforces the depth and key limits of
/// `HasLoadLimits`, and records spans in node metadata when the backend reports them.
#[cgp_component(FormatParser)]
#[derive_delegate(UseDelegate<Format>)]
pub trait CanParseFormat<Format>: HasConfigValueType + HasErrorType {
    /// Parses `text`, read from `origin`.
    ///
    /// # Errors
    ///
    /// `ConfigError::Parse` (raised into the context error) for invalid text,
    /// `DuplicateKey`, or `LimitExceeded`.
    fn parse_format(
        &self,
        format: PhantomData<Format>,
        origin: &Origin,
        text: &str,
    ) -> Result<Self::ConfigValue, Self::Error>;
}

/// Renders a tree in the format identified by the marker type `Format` (templates, dumps,
/// migrations).
#[cgp_component(FormatRenderer)]
#[derive_delegate(UseDelegate<Format>)]
pub trait CanRenderFormat<Format>: HasConfigValueType + HasErrorType {
    /// Renders `value`.
    ///
    /// # Errors
    ///
    /// When the tree cannot be expressed in the format.
    fn render_format(
        &self,
        format: PhantomData<Format>,
        value: &Self::ConfigValue,
    ) -> Result<String, Self::Error>;
}
