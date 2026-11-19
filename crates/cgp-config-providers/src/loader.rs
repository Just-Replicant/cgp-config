//! Ready-made loader contexts: [`StdLoader`] (files and env, `TwelveFactorPreset`) and
//! [`MemoryLoader`] (documents in memory, no process IO), plus the [`from_toml_str`],
//! [`from_json_str`], and [`from_defaults`] shortcuts.
//!
//! Both contexts validate with `Nested` (the derived `ValidateSections`), so the config
//! type is a `#[derive(ConfigSection)]` type; contexts with their own validators or
//! another preset are a few lines of `delegate_components!` (see the crate docs).

use std::path::PathBuf;

use cgp_config_core::components::*;
use cgp_config_core::markers::{Defaults, EnvVars, InMemory};
use cgp_config_core::{
    ConfigError, ConfigMeta, ConfigMetaField, ConfigSection, EnvSnapshot, IdentityLayer,
    LoadReport, Nested,
};
#[cfg(any(feature = "toml", feature = "json"))]
use cgp_config_core::{LoadLimits, Origin};
use cgp_config_value::{ConfigValue, CoreConfigPreset, PrepareFileLayer};
use cgp_fork::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_fork::extra::error::RaiseFrom;
use cgp_fork::prelude::*;

use crate::bootstrap::Bootstrapped;
use crate::location::ConfigLocation;
use crate::presets::{TestConfigPreset, TwelveFactorPreset};
use crate::read::ConfigFileReaderComponent;
use crate::sources::CliOverrides;

/// A loader for applications: `Defaults`, the config file, the profile file, and env,
/// from a [`Bootstrapped`] (see [`Bootstrap`](crate::bootstrap::Bootstrap)).
///
/// ```no_run
/// # use cgp_config_core::ConfigSection;
/// # fn demo<AppConfig>() -> Result<(), cgp_config_core::ConfigError>
/// # where AppConfig: ConfigSection, cgp_config_providers::loader::StdLoader: cgp_config_core::CanLoadConfig<AppConfig> {
/// use cgp_config_core::CanLoadConfig;
/// use cgp_config_providers::bootstrap::Bootstrap;
/// use cgp_config_providers::loader::StdLoader;
///
/// let boot = Bootstrap::new("my-app").capture_env()?.from_args(std::env::args())?.finish();
/// let loader = StdLoader::for_section::<AppConfig>(boot);
/// let config: AppConfig = loader.load_config()?;
/// for warning in loader.load_report.warnings() {
///     eprintln!("config: {warning}");
/// }
/// # Ok(()) }
/// ```
#[derive(Debug, HasField)]
pub struct StdLoader {
    /// The env snapshot (bootstrap keys removed).
    pub env_snapshot: EnvSnapshot,
    /// The env prefix.
    pub env_prefix: String,
    /// The active profile.
    pub active_profile: String,
    /// The main config file.
    pub config_location: ConfigLocation,
    /// The root schema.
    pub config_meta: Option<&'static ConfigMeta>,
    /// Warnings of the last load.
    pub load_report: LoadReport,
}

impl StdLoader {
    /// A loader for `bootstrapped`, without a schema (plain serde types).
    #[must_use]
    pub fn new(bootstrapped: Bootstrapped) -> Self {
        Self {
            env_snapshot: bootstrapped.env_snapshot,
            env_prefix: bootstrapped.env_prefix,
            active_profile: bootstrapped.active_profile,
            config_location: bootstrapped.config_location,
            config_meta: None,
            load_report: LoadReport::new(),
        }
    }

    /// A loader for `bootstrapped` with the schema of `Config`.
    #[must_use]
    pub fn for_section<Config: ConfigSection>(bootstrapped: Bootstrapped) -> Self {
        Self {
            config_meta: Some(Config::META),
            ..Self::new(bootstrapped)
        }
    }

    /// Loads `Config` (`load_config` with the type named).
    ///
    /// # Errors
    ///
    /// Any `ConfigError` of the pipeline.
    pub fn load<Config>(&self) -> Result<Config, ConfigError>
    where
        Self: CanLoadConfig<Config, Error = ConfigError>,
    {
        self.load_config()
    }
}

delegate_components! {
    #[unchecked]
    StdLoader {
        preset TwelveFactorPreset,
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        RootConfigMetaComponent: ConfigMetaField,
        ConfigValidatorComponent: Nested,
    }
}

/// A loader over one in-memory document: `Defaults`, the document (prepared like a file,
/// so `$profiles` and `$version` apply), and an env snapshot (empty unless set). It never
/// touches the filesystem or the process environment.
#[derive(Debug, HasField)]
pub struct MemoryLoader {
    /// The document.
    pub memory_layer: ConfigValue,
    /// The env snapshot.
    pub env_snapshot: EnvSnapshot,
    /// The env prefix for `env_snapshot` (default `APP`).
    pub env_prefix: String,
    /// The active profile (default `default`).
    pub active_profile: String,
    /// The root schema.
    pub config_meta: Option<&'static ConfigMeta>,
    /// Warnings of the last load.
    pub load_report: LoadReport,
    // `TwelveFactorPreset` wires the std file reader; the memory loader has no files, so
    // its location is always empty.
    config_location: ConfigLocation,
    cli_overrides: CliOverrides,
    dotenv_path: Option<PathBuf>,
}

impl MemoryLoader {
    /// A loader for `layer` with the schema of `Config`.
    #[must_use]
    pub fn new<Config: ConfigSection>(layer: ConfigValue) -> Self {
        Self {
            memory_layer: layer,
            env_snapshot: EnvSnapshot::new(),
            env_prefix: "APP".to_owned(),
            active_profile: "default".to_owned(),
            config_meta: Some(Config::META),
            load_report: LoadReport::new(),
            config_location: ConfigLocation::none(),
            cli_overrides: CliOverrides::new(),
            dotenv_path: None,
        }
    }

    /// A loader with only the defaults of `Config`.
    #[must_use]
    pub fn defaults<Config: ConfigSection>() -> Self {
        Self::new::<Config>(ConfigValue::empty_map())
    }

    /// A loader for TOML `text` (feature `toml`).
    ///
    /// # Errors
    ///
    /// `ConfigError::Parse` with `line:column`.
    #[cfg(feature = "toml")]
    pub fn from_toml<Config: ConfigSection>(text: &str) -> Result<Self, ConfigError> {
        let origin = Origin::memory("toml");
        let tree = cgp_config_formats::parse_toml(&origin, text, &LoadLimits::DEFAULT)?;
        Ok(Self::new::<Config>(root_table(tree)?))
    }

    /// A loader for JSON `text` (feature `json`).
    ///
    /// # Errors
    ///
    /// `ConfigError::Parse` with `line:column`.
    #[cfg(feature = "json")]
    pub fn from_json<Config: ConfigSection>(text: &str) -> Result<Self, ConfigError> {
        let origin = Origin::memory("json");
        let tree = cgp_config_formats::parse_json(&origin, text, &LoadLimits::DEFAULT)?;
        Ok(Self::new::<Config>(root_table(tree)?))
    }

    /// Sets the env snapshot and prefix (for tests of env mapping).
    #[must_use]
    pub fn with_env(mut self, prefix: impl Into<String>, env: EnvSnapshot) -> Self {
        self.env_prefix = prefix.into();
        self.env_snapshot = env;
        self
    }

    /// Sets the active profile.
    #[must_use]
    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.active_profile = profile.into();
        self
    }

    /// Loads `Config`.
    ///
    /// # Errors
    ///
    /// Any `ConfigError` of the pipeline.
    pub fn load<Config>(&self) -> Result<Config, ConfigError>
    where
        Self: CanLoadConfig<Config, Error = ConfigError>,
    {
        self.load_config()
    }
}

#[cfg(any(feature = "toml", feature = "json"))]
fn root_table(tree: ConfigValue) -> Result<ConfigValue, ConfigError> {
    let file = crate::read::FileText::new(std::path::Path::new(""), String::new());
    crate::file::finish_document(tree, &file, &mut |_| {})
}

/// The preparers of [`MemoryLoader`]: its document is a file.
pub struct MemoryPreparers;

delegate_components! {
    #[unchecked]
    MemoryPreparers {
        InMemory: PrepareFileLayer,
        [Defaults, EnvVars]: IdentityLayer,
    }
}

delegate_components! {
    #[unchecked]
    MemoryLoader {
        preset TestConfigPreset,
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        RootConfigMetaComponent: ConfigMetaField,
        ConfigValidatorComponent: Nested,
        LayerPreparerComponent: UseDelegate<MemoryPreparers>,
    }
}

// `ConfigFileReaderComponent` is replayed by the presets above.
const _: Option<ConfigFileReaderComponent> = None;

/// Loads `Config` from TOML text: defaults, then the document; deserialized, then
/// validated. No process IO. Warnings (unknown keys) are dropped; use
/// [`MemoryLoader::from_toml`] to read them.
///
/// # Errors
///
/// Any `ConfigError`: syntax (with `line:column`), types, missing fields, validation.
#[cfg(feature = "toml")]
pub fn from_toml_str<Config>(text: &str) -> Result<Config, ConfigError>
where
    Config: ConfigSection,
    MemoryLoader: CanLoadConfig<Config, Error = ConfigError>,
{
    MemoryLoader::from_toml::<Config>(text)?.load()
}

/// Loads `Config` from JSON text, as [`from_toml_str`].
///
/// # Errors
///
/// As [`from_toml_str`].
#[cfg(feature = "json")]
pub fn from_json_str<Config>(text: &str) -> Result<Config, ConfigError>
where
    Config: ConfigSection,
    MemoryLoader: CanLoadConfig<Config, Error = ConfigError>,
{
    MemoryLoader::from_json::<Config>(text)?.load()
}

/// `Config` from its `#[config(default = ...)]` values alone.
///
/// # Errors
///
/// `ConfigError::Missing` for required fields without a default, and validation errors.
pub fn from_defaults<Config>() -> Result<Config, ConfigError>
where
    Config: ConfigSection,
    MemoryLoader: CanLoadConfig<Config, Error = ConfigError>,
{
    MemoryLoader::defaults::<Config>().load()
}

/// `AppConfig::from_defaults()`, `AppConfig::from_toml_str(..)`: the shortcuts as
/// methods of every section type.
pub trait LoadSection: ConfigSection + Sized
where
    MemoryLoader: CanLoadConfig<Self, Error = ConfigError>,
{
    /// See [`from_defaults`].
    ///
    /// # Errors
    ///
    /// As [`from_defaults`].
    fn from_defaults() -> Result<Self, ConfigError> {
        from_defaults()
    }

    /// See [`from_toml_str`].
    ///
    /// # Errors
    ///
    /// As [`from_toml_str`].
    #[cfg(feature = "toml")]
    fn from_toml_str(text: &str) -> Result<Self, ConfigError> {
        from_toml_str(text)
    }

    /// See [`from_json_str`].
    ///
    /// # Errors
    ///
    /// As [`from_json_str`].
    #[cfg(feature = "json")]
    fn from_json_str(text: &str) -> Result<Self, ConfigError> {
        from_json_str(text)
    }
}

impl<Config: ConfigSection> LoadSection for Config where
    MemoryLoader: CanLoadConfig<Config, Error = ConfigError>
{
}
