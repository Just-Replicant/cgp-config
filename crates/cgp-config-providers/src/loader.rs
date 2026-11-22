//! Ready-made loader contexts: [`StdLoader`] (files and env, `TwelveFactorPreset`) and
//! [`MemoryLoader`] (documents in memory, no process IO), plus the [`from_toml_str`],
//! [`from_json_str`], and [`from_defaults`] shortcuts.
//!
//! Both contexts validate with `Nested` (the derived `ValidateSections`), so the config
//! type is a `#[derive(ConfigSection)]` type; contexts with their own validators or
//! another preset are a few lines of `delegate_components!` (see the crate docs).

use core::marker::PhantomData;
use std::path::PathBuf;

use cgp_config_core::components::*;
use cgp_config_core::markers::{Defaults, EnvVars, InMemory};
use cgp_config_core::{
    ConfigError, ConfigLoaded, ConfigMeta, ConfigMetaField, ConfigSection, EnvSnapshot,
    IdentityLayer, LoadLayered, LoadReport, LoadStamp, LoadWarning, Nested,
};
#[cfg(any(feature = "toml", feature = "json"))]
use cgp_config_core::{LoadLimits, Origin};
use cgp_config_value::{ConfigValue, CoreConfigPreset, PrepareFileLayer};
use cgp_fork::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_fork::extra::error::RaiseFrom;
use cgp_fork::prelude::*;

use crate::bootstrap::Bootstrapped;
use crate::layers::{LayerList, TestLayers, TwelveFactorLayers};
use crate::location::ConfigLocation;
use crate::presets::{TestConfigPreset, TwelveFactorPreset};
use crate::read::ConfigFileReaderComponent;
use crate::sources::CliOverrides;

/// A loader for applications, from a [`Bootstrapped`] (see
/// [`Bootstrap`](crate::bootstrap::Bootstrap)): `TwelveFactorPreset` with the layer list
/// `Layers` (default [`TwelveFactorLayers`]: `Defaults`, the config file, the profile
/// file, env).
///
/// It keeps every bootstrap input (the `{PREFIX}_DOCUMENT` text, the `.env` path, the
/// `--set` overrides), so any standard list works: `StdLoader<TwelveFactorCliLayers>`
/// reads `--set`, `StdLoader<TwelveFactorDotenvLayers>` reads `.env` (feature `dotenv`),
/// `StdLoader<TwelveFactorDocumentLayers>` reads `{PREFIX}_DOCUMENT` (feature `json`).
/// Construction fails when a bootstrap input would be read by no layer of the list
/// ([`Bootstrapped::check_layers`]) instead of dropping it.
///
/// ```no_run
/// # use cgp_config_core::ConfigSection;
/// # fn demo<AppConfig>() -> Result<(), cgp_config_core::ConfigError>
/// # where AppConfig: ConfigSection, cgp_config_providers::loader::StdLoader<cgp_config_providers::layers::TwelveFactorCliLayers>: cgp_config_core::CanLoadConfig<AppConfig> {
/// use cgp_config_core::CanLoadConfig;
/// use cgp_config_providers::bootstrap::Bootstrap;
/// use cgp_config_providers::layers::TwelveFactorCliLayers;
/// use cgp_config_providers::loader::StdLoader;
///
/// let boot = Bootstrap::new("my-app")
///     .layers::<TwelveFactorCliLayers>()
///     .capture_env()?
///     .from_args(std::env::args())?
///     .finish();
/// let loader = StdLoader::<TwelveFactorCliLayers>::with_layers(boot)?.with_section::<AppConfig>();
/// let config: AppConfig = loader.load_config()?;
/// for warning in loader.load_report.warnings() {
///     eprintln!("config: {warning}");
/// }
/// # Ok(()) }
/// ```
#[derive(HasField)]
pub struct StdLoader<Layers = TwelveFactorLayers> {
    /// The env snapshot (bootstrap keys removed).
    pub env_snapshot: EnvSnapshot,
    /// The env prefix.
    pub env_prefix: String,
    /// The active profile.
    pub active_profile: String,
    /// The main config file.
    pub config_location: ConfigLocation,
    /// The `{PREFIX}_DOCUMENT` text (the `EnvDocument` layer).
    pub env_document: Option<String>,
    /// The `.env` file (the `DotEnv` layer).
    pub dotenv_path: Option<PathBuf>,
    /// The `--set` overrides (the `CliArgs` layer).
    pub cli_overrides: CliOverrides,
    /// The root schema.
    pub config_meta: Option<&'static ConfigMeta>,
    /// Warnings of the last load.
    pub load_report: LoadReport,
    layers: PhantomData<fn() -> Layers>,
}

impl<Layers> core::fmt::Debug for StdLoader<Layers> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StdLoader")
            .field("env_snapshot", &self.env_snapshot)
            .field("env_prefix", &self.env_prefix)
            .field("active_profile", &self.active_profile)
            .field("config_location", &self.config_location)
            .field("env_document", &self.env_document.as_ref().map(|_| "<set>"))
            .field("dotenv_path", &self.dotenv_path)
            .field("cli_overrides", &self.cli_overrides)
            .field("config_meta", &self.config_meta.map(|meta| meta.name))
            .field("load_report", &self.load_report)
            .finish()
    }
}

impl StdLoader {
    /// Starts a [`LoaderBuilder`]. The builder does not read the process environment
    /// until [`LoaderBuilder::capture_env`].
    #[must_use]
    pub fn builder() -> LoaderBuilder {
        LoaderBuilder::default()
    }

    /// A twelve-factor loader for `bootstrapped`, without a schema (plain serde types).
    ///
    /// # Errors
    ///
    /// As [`Bootstrapped::check_layers`]: `--set` overrides, a `{PREFIX}_DOCUMENT`, or an
    /// explicit `.env` path, none of which the twelve-factor layers read.
    pub fn new(bootstrapped: Bootstrapped) -> Result<Self, ConfigError> {
        Self::with_layers(bootstrapped)
    }

    /// A twelve-factor loader for `bootstrapped` with the schema of `Config`.
    ///
    /// # Errors
    ///
    /// As [`new`](Self::new).
    pub fn for_section<Config: ConfigSection>(
        bootstrapped: Bootstrapped,
    ) -> Result<Self, ConfigError> {
        Ok(Self::new(bootstrapped)?.with_section::<Config>())
    }
}

impl<Layers: LayerList> StdLoader<Layers> {
    /// A loader for `bootstrapped` reading the layers `Layers`, without a schema.
    ///
    /// # Errors
    ///
    /// As [`Bootstrapped::check_layers`].
    pub fn with_layers(bootstrapped: Bootstrapped) -> Result<Self, ConfigError> {
        let bootstrapped = bootstrapped.check_layers::<Layers>()?;
        Ok(Self {
            env_snapshot: bootstrapped.env_snapshot,
            env_prefix: bootstrapped.env_prefix,
            active_profile: bootstrapped.active_profile,
            config_location: bootstrapped.config_location,
            env_document: bootstrapped.env_document,
            dotenv_path: bootstrapped.dotenv_path,
            cli_overrides: bootstrapped.cli_overrides,
            config_meta: None,
            load_report: LoadReport::new(),
            layers: PhantomData,
        })
    }
}

impl<Layers> StdLoader<Layers> {
    /// Uses the schema of `Config` (defaults, env mapping, schema rules).
    #[must_use]
    pub fn with_section<Config: ConfigSection>(mut self) -> Self {
        self.config_meta = Some(Config::META);
        self
    }

    /// Loads `Config` (`load_config` with the type named).
    ///
    /// # Errors
    ///
    /// Any `ConfigError` of the pipeline.
    pub fn load<Config>(&self) -> Result<Config, ConfigError>
    where
        Self: CanLoadConfig<Config, Error = ConfigError> + CanLog<ConfigLoaded>,
    {
        let config = self.load_config()?;
        log_loaded(self);
        Ok(config)
    }
}

pub(crate) fn log_loaded<Context>(context: &Context)
where
    Context: CanLog<ConfigLoaded> + HasField<Symbol!("load_report"), Value = LoadReport>,
{
    if let Some(stamp) = context
        .get_field(PhantomData::<Symbol!("load_report")>)
        .stamp()
    {
        context.log(ConfigLoaded::new(stamp.origin, stamp.fingerprint));
    }
}

delegate_components! {
    #[unchecked]
    <Layers> StdLoader<Layers> {
        preset TwelveFactorPreset,
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        RootConfigMetaComponent: ConfigMetaField,
        ConfigValidatorComponent: Nested,
        ConfigLoaderComponent: LoadLayered<Layers>,
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
        Self: CanLoadConfig<Config, Error = ConfigError> + CanLog<ConfigLoaded>,
    {
        let config = self.load_config()?;
        log_loaded(self);
        Ok(config)
    }

    /// Loads `Config` through the loader's stages (collect, interpolate, deserialize,
    /// the preset secrets stage), then `resolver` (the caller's secret resolution), then
    /// validation, and returns the config with the warnings of the load.
    ///
    /// # Errors
    ///
    /// Any `ConfigError` of the pipeline or of `resolver`.
    pub fn load_with<Config>(
        &self,
        resolver: impl FnOnce(&mut Config) -> Result<(), ConfigError>,
    ) -> Result<Loaded<Config>, ConfigError>
    where
        Self: HasEmptyConfigValue
            + CanReportWarning
            + CanCollectLayers<TestLayers>
            + CanInterpolate<Config>
            + CanDeserializeConfig<Config>
            + CanResolveSecrets<Config>
            + CanValidateConfig<Config>
            + CanOpenSecretSession
            + HasErrorType<Error = ConfigError>,
    {
        self.clear_warnings();
        let tree = self.collect_layers(self.empty_config_value(), PhantomData::<TestLayers>)?;
        let tree = self.interpolate(tree)?;
        let _session = self.open_secret_session();
        let mut config = self.deserialize_config(tree)?;
        self.resolve_secrets(&mut config)?;
        resolver(&mut config)?;
        self.validate_config(&config)?;
        Ok(Loaded {
            config,
            warnings: self.load_report.warnings(),
            stamp: self.load_report.stamp(),
        })
    }
}

/// A config with the warnings of the load that produced it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Loaded<Config> {
    /// The config.
    pub config: Config,
    /// The warnings, in order (unknown keys, deprecated names, ...).
    pub warnings: Vec<LoadWarning>,
    /// The redacted stamp (fingerprint, explain, dump), when deserialization recorded one.
    pub stamp: Option<LoadStamp>,
}

impl<Config> Loaded<Config> {
    /// FNV-1a 64 of the canonical redacted JSON.
    #[must_use]
    pub fn fingerprint(&self) -> Option<u64> {
        self.stamp.as_ref().map(|stamp| stamp.fingerprint)
    }

    /// The explanation of `path`.
    #[must_use]
    pub fn explain(&self, path: &str) -> Option<String> {
        self.stamp
            .as_ref()
            .and_then(|stamp| stamp.explain(path))
            .map(|explanation| explanation.to_string())
    }

    /// Canonical redacted JSON.
    #[must_use]
    pub fn dump_json(&self) -> Option<&str> {
        self.stamp.as_ref().map(|stamp| stamp.dump_json.as_str())
    }
}

#[cfg(any(feature = "toml", feature = "json"))]
fn root_table(tree: ConfigValue) -> Result<ConfigValue, ConfigError> {
    let file = crate::read::FileText::new(std::path::Path::new(""), String::new());
    crate::file::finish_document(tree, &file, None, &mut |_| {})
}

/// TOML of the canonical redacted JSON, when a stamp was recorded.
///
/// # Errors
///
/// The redacted JSON is not a document the TOML renderer can print.
#[cfg(all(feature = "toml", feature = "json"))]
pub fn dump_toml(report: &LoadReport) -> Result<Option<String>, ConfigError> {
    let Some(json) = report.dump_json() else {
        return Ok(None);
    };
    let value =
        cgp_config_formats::parse_json(&Origin::memory("redacted"), &json, &LoadLimits::UNLIMITED)?;
    cgp_config_formats::render_toml(&value).map(Some)
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

/// Loads `Config` from TOML text: defaults, then the document; deserialized, the
/// preset secrets stage, then validated: the stages of a loader. No process IO.
/// Warnings (unknown keys) are dropped; [`from_toml_str_with`] returns them and takes a
/// secret resolver.
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

/// [`from_toml_str`] with the caller's secret resolution and the warnings: `resolver` runs
/// after deserialization and before validation, as the secrets stage of a loader does.
///
/// # Errors
///
/// As [`from_toml_str`], and any error of `resolver`.
#[cfg(feature = "toml")]
pub fn from_toml_str_with<Config>(
    text: &str,
    resolver: impl FnOnce(&mut Config) -> Result<(), ConfigError>,
) -> Result<Loaded<Config>, ConfigError>
where
    Config: ConfigSection,
    MemoryLoader: CanInterpolate<Config>
        + CanDeserializeConfig<Config>
        + CanResolveSecrets<Config>
        + CanValidateConfig<Config>,
{
    MemoryLoader::from_toml::<Config>(text)?.load_with(resolver)
}

/// [`from_json_str`] with the caller's secret resolution and the warnings, as
/// [`from_toml_str_with`].
///
/// # Errors
///
/// As [`from_toml_str_with`].
#[cfg(feature = "json")]
pub fn from_json_str_with<Config>(
    text: &str,
    resolver: impl FnOnce(&mut Config) -> Result<(), ConfigError>,
) -> Result<Loaded<Config>, ConfigError>
where
    Config: ConfigSection,
    MemoryLoader: CanInterpolate<Config>
        + CanDeserializeConfig<Config>
        + CanResolveSecrets<Config>
        + CanValidateConfig<Config>,
{
    MemoryLoader::from_json::<Config>(text)?.load_with(resolver)
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

/// Fills a [`StdLoader`] without naming a CGP type.
///
/// The env prefix has no trailing separator: `APP`, so a nested key is `APP__HTTP__PORT`.
/// `APP_` and `APP__` are rejected. The process environment is read only after
/// [`capture_env`](Self::capture_env); otherwise pass [`env`](Self::env) or leave the
/// snapshot empty.
///
/// ```
/// use cgp_config_core::{ConfigMeta, ConfigSection, EnvSnapshot};
/// use cgp_config_providers::loader::StdLoader;
///
/// struct App;
/// impl ConfigSection for App {
///     const META: &'static ConfigMeta = &ConfigMeta::new("App", &[]);
/// }
///
/// let loader = StdLoader::builder()
///     .app_name("demo")
///     .file("cfg.toml")
///     .env_prefix("DEMO")
///     .profile("prod")
///     .env(EnvSnapshot::new())
///     .build::<App>()?;
/// assert_eq!(loader.env_prefix, "DEMO");
/// assert_eq!(loader.active_profile, "prod");
/// assert_eq!(
///     loader.config_location.file.as_deref().and_then(|path| path.to_str()),
///     Some("cfg.toml")
/// );
/// # Ok::<(), cgp_config_core::ConfigError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct LoaderBuilder {
    app: Option<String>,
    prefix: Option<String>,
    file: Option<PathBuf>,
    profile: Option<String>,
    env: Option<EnvSnapshot>,
    capture: bool,
    require_file: bool,
}

impl LoaderBuilder {
    /// The `{app}` segment of the search and the default env prefix.
    #[must_use]
    pub fn app_name(mut self, app: impl Into<String>) -> Self {
        self.app = Some(app.into());
        self
    }

    /// An explicit config file (`--config`). It is required to exist at load time.
    #[must_use]
    pub fn file(mut self, path: impl Into<PathBuf>) -> Self {
        self.file = Some(path.into());
        self
    }

    /// The env prefix, with no trailing `_` (`APP`, not `APP_`).
    #[must_use]
    pub fn env_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// The active profile (default `default`).
    #[must_use]
    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// Uses `env` as the snapshot. Cancels [`capture_env`](Self::capture_env).
    #[must_use]
    pub fn env(mut self, env: EnvSnapshot) -> Self {
        self.capture = false;
        self.env = Some(env);
        self
    }

    /// Reads the process environment when [`build`](Self::build) runs.
    #[must_use]
    pub fn capture_env(mut self) -> Self {
        self.capture = true;
        self.env = None;
        self
    }

    /// Makes a search that finds no file an error. An explicit [`file`](Self::file) is
    /// already required.
    #[must_use]
    pub fn require_file(mut self, required: bool) -> Self {
        self.require_file = required;
        self
    }

    /// Builds a twelve-factor [`StdLoader`] for `Config`.
    ///
    /// # Errors
    ///
    /// `app_name` was not set, the prefix ends with `_`, or capturing the process
    /// environment failed.
    pub fn build<Config: ConfigSection>(self) -> Result<StdLoader, ConfigError> {
        let app = self.app.ok_or_else(|| {
            cgp_config_core::ConfigError::parse(
                cgp_config_core::Origin::cli("app_name"),
                None,
                "`LoaderBuilder` needs `app_name`",
            )
        })?;
        let mut boot = crate::bootstrap::Bootstrap::new(app);
        if let Some(prefix) = self.prefix {
            check_prefix(&prefix)?;
            boot = boot.env_prefix(prefix);
        }
        if let Some(file) = self.file {
            boot = boot.config_path(file);
        }
        if let Some(profile) = self.profile {
            boot = boot.profile(profile);
        }
        boot = boot.require_file(self.require_file);
        if self.capture {
            boot = boot.capture_env()?;
        } else if let Some(env) = self.env {
            boot = boot.with_env(env);
        }
        StdLoader::for_section::<Config>(boot.finish())
    }
}

fn check_prefix(prefix: &str) -> Result<(), ConfigError> {
    if prefix.is_empty() || prefix.ends_with('_') {
        return Err(ConfigError::parse(
            cgp_config_core::Origin::cli("--env-prefix"),
            None,
            "an env prefix has no trailing separator (`APP`, not `APP_` or `APP__`); \
             nested keys are `{PREFIX}__{PATH}`",
        ));
    }
    Ok(())
}
