//! Format markers and run-time dispatch by extension.

use alloc::string::String;

use cgp_config_core::{ConfigError, LoadLimits, Origin};
use cgp_config_value::ConfigValue;

/// A document format: its name and file extensions.
pub trait FileFormat {
    /// The format name (`toml`, `json`).
    const NAME: &'static str;
    /// Extensions, without the dot, lowercase.
    const EXTENSIONS: &'static [&'static str];
    /// The run-time kind.
    const KIND: FormatKind;
}

/// TOML 1.1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Toml;

/// JSON (RFC 8259).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Json;

impl FileFormat for Toml {
    const NAME: &'static str = "toml";
    const EXTENSIONS: &'static [&'static str] = &["toml"];
    const KIND: FormatKind = FormatKind::Toml;
}

impl FileFormat for Json {
    const NAME: &'static str = "json";
    const EXTENSIONS: &'static [&'static str] = &["json"];
    const KIND: FormatKind = FormatKind::Json;
}

/// A format chosen at run time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FormatKind {
    /// TOML.
    Toml,
    /// JSON.
    Json,
}

impl FormatKind {
    /// Every format this crate knows, compiled in or not, in search priority order.
    pub const ALL: [Self; 2] = [Self::Toml, Self::Json];

    /// The format name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Toml => Toml::NAME,
            Self::Json => Json::NAME,
        }
    }

    /// The extensions of the format.
    #[must_use]
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Toml => Toml::EXTENSIONS,
            Self::Json => Json::EXTENSIONS,
        }
    }

    /// Whether the format's feature is enabled in this build.
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        match self {
            Self::Toml => cfg!(feature = "toml"),
            Self::Json => cfg!(feature = "json"),
        }
    }

    /// The enabled formats, in priority order (TOML first).
    pub fn enabled() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter().filter(|kind| kind.is_enabled())
    }

    /// The format of a file extension (ASCII case-insensitive), compiled in or not.
    ///
    /// ```
    /// use cgp_config_formats::FormatKind;
    ///
    /// assert_eq!(FormatKind::from_extension("TOML"), Some(FormatKind::Toml));
    /// assert_eq!(FormatKind::from_extension("yaml"), None);
    /// ```
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| {
            kind.extensions()
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
    }

    /// The format of a path by its last extension.
    #[must_use]
    pub fn from_path(path: &str) -> Option<Self> {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let (_, extension) = name.rsplit_once('.')?;
        Self::from_extension(extension)
    }
}

/// Parses `text` as `kind`.
///
/// # Errors
///
/// `ConfigError::Parse` for invalid text, or when the format is not compiled in
/// (naming the feature to enable); `DuplicateKey` and `LimitExceeded` as the parsers
/// report them.
pub fn parse_document(
    kind: FormatKind,
    origin: &Origin,
    text: &str,
    limits: &LoadLimits,
) -> Result<ConfigValue, ConfigError> {
    match kind {
        #[cfg(feature = "toml")]
        FormatKind::Toml => crate::parse_toml(origin, text, limits),
        #[cfg(feature = "json")]
        FormatKind::Json => crate::parse_json(origin, text, limits),
        #[allow(unreachable_patterns)]
        other => {
            let _ = (text, limits);
            Err(disabled(origin, other))
        }
    }
}

/// Renders a redacted copy of `value` as `kind`.
///
/// # Errors
///
/// When the tree cannot be expressed in the format (TOML has no null), or the format is
/// not compiled in.
pub fn render_document(kind: FormatKind, value: &ConfigValue) -> Result<String, ConfigError> {
    match kind {
        #[cfg(feature = "toml")]
        FormatKind::Toml => crate::render_toml(value),
        FormatKind::Json => render_json(value),
        #[allow(unreachable_patterns)]
        other => Err(disabled(&Origin::Unknown, other)),
    }
}

/// JSON rendering needs no parser, so it is always available.
pub(crate) fn render_json(value: &ConfigValue) -> Result<String, ConfigError> {
    serde_json::to_string_pretty(&value.redacted()).map_err(|error| {
        ConfigError::parse(
            Origin::Unknown,
            None,
            alloc::format!("cannot render JSON: {error}"),
        )
    })
}

#[allow(dead_code)]
fn disabled(origin: &Origin, kind: FormatKind) -> ConfigError {
    ConfigError::parse(
        origin.clone(),
        None,
        alloc::format!(
            "the {} format is not enabled; enable the `{}` feature",
            kind.name(),
            kind.name()
        ),
    )
}
