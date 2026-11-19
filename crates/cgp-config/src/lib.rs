//! Context-generic, layered, typed configuration for Rust, built on
//! [CGP](https://contextgeneric.dev/).
//!
//! A *context* (a plain struct of yours) decides at compile time where configuration
//! comes from, how layers merge, how values deserialize, and how the result is checked.
//! Every stage is a CGP component with swappable providers; presets wire sensible
//! defaults for all of them.
//!
//! # Example
//!
//! (Needs the default features `std`, `derive`, and `toml`.)
//!
#![cfg_attr(
    all(feature = "std", feature = "derive", feature = "toml"),
    doc = "```"
)]
#![cfg_attr(
    not(all(feature = "std", feature = "derive", feature = "toml")),
    doc = "```ignore"
)]
//! use std::time::Duration;
//!
//! use cgp_config::prelude::*;
//! use cgp_config::cgp::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
//! use cgp_config::cgp::extra::error::RaiseFrom;
//!
//! /// The application's configuration.
//! #[derive(Debug, serde::Deserialize, ConfigSection)]
//! struct AppConfig {
//!     #[config(default = "demo")]
//!     name: String,
//!     #[config(nested)]
//!     http: Http,
//! }
//!
//! #[derive(Debug, serde::Deserialize, ConfigSection)]
//! struct Http {
//!     /// Port to listen on.
//!     #[config(default = 8080, env_raw = "PORT")]
//!     port: u16,
//!     #[config(default = "30s", parse = "duration")]
//!     timeout: Duration,
//! }
//!
//! // Tests and embedded documents: no process IO.
//! let config: AppConfig = cgp_config::from_toml_str("[http]\nport = 9090\n")?;
//! assert_eq!(config.http.port, 9090);
//! assert_eq!(config.http.timeout, Duration::from_secs(30));
//!
//! // An application loader: defaults, the config file, the profile file, env.
//! #[derive(HasField)]
//! struct Loader {
//!     env_snapshot: EnvSnapshot,
//!     env_prefix: String,
//!     active_profile: String,
//!     config_location: ConfigLocation,
//!     load_report: LoadReport,
//! }
//!
//! delegate_components! {
//!     #[unchecked]
//!     Loader {
//!         preset TwelveFactorPreset,
//!         ErrorTypeProviderComponent: UseType<ConfigError>,
//!         ErrorRaiserComponent: RaiseFrom,
//!         RootConfigMetaComponent: UseConfigMeta<AppConfig>,
//!         ConfigValidatorComponent: Nested,
//!     }
//! }
//!
//! config_check! {
//!     Loader { config: AppConfig, sources: [Defaults, ConfigFile, ProfileFile, EnvVars] }
//! }
//!
//! // Real applications call `Bootstrap::new("demo").capture_env()?`; this example
//! // supplies the environment and searches no directory.
//! let boot = Bootstrap::new("demo")
//!     .with_env(EnvSnapshot::from_iter([("DEMO__HTTP__PORT", "7000")]))
//!     .search(SearchPath::new("demo"))
//!     .finish();
//! let loader = Loader {
//!     env_snapshot: boot.env_snapshot,
//!     env_prefix: boot.env_prefix,
//!     active_profile: boot.active_profile,
//!     config_location: boot.config_location,
//!     load_report: LoadReport::new(),
//! };
//! let config: AppConfig = loader.load_config()?;
//! assert_eq!((config.name.as_str(), config.http.port), ("demo", 7000));
//! # Ok::<(), ConfigError>(())
//! ```
//!
//! An application depends on `cgp-config` and on `cgp-fork` directly (the CGP macros
//! name `::cgp_fork` in their expansion), and imports [`prelude`]. The prelude brings
//! every component and preset name into scope, which presets need: `preset
//! TwelveFactorPreset` replays its component keys at the use site.
//!
//! # Layers
//!
//! `TwelveFactorPreset` loads, low to high: `#[config(default)]` values, the config file
//! (`--config`, `{PREFIX}_CONFIG`, or the first of `$CONFIGURATION_DIRECTORY/{app}.toml`,
//! `./{app}.toml`, `./config.toml`, `./config/{app}.toml`, the same beside the
//! executable, and the platform config directory), `{app}.{profile}.toml` beside it, and
//! `{PREFIX}__SECTION__KEY` env variables. Child presets add `.env` (`TwelveFactorDotenv`),
//! `--set key=value` (`TwelveFactorCli`), `{app}.local.toml` (`TwelveFactorLocal`), and
//! `conf.d/` (`TwelveFactorConfDir`); see [`presets`].
//!
//! # Features
//!
//! | Feature | Default | Enables |
//! |---|---|---|
//! | `std` | yes | `alloc`, std sources and presets ([`bootstrap`], [`presets`], [`loader`]) |
//! | `derive` | yes | `#[derive(ConfigSection)]` (see [`derive`](mod@derive)) |
//! | `toml` | yes | TOML documents |
//! | `json` | no | JSON documents and `{PREFIX}_DOCUMENT` |
//! | `alloc` | via `std` | the value tree, merge, deserializer, errors, `CoreConfigPreset` |
//! | `dotenv` | no | `.env` files (`TwelveFactorDotenv`) |
//! | `clap` | no | `--config`, `--profile`, `--set` for clap (`TwelveFactorCli`) |
//! | `interpolate` | no | `${VAR}` / `${path}` in opted-in fields (`InterpolatingPreset`) |
//! | `validate-garde`, `validate-validator` | no | `ValidateWithGarde`, `ValidateWithValidator` |
//! | `anyhow`, `eyre`, `std-error` | no | the fork's error-type providers |
//! | `tokio`, `smol` | no | the fork's async runtime providers (used by reload in 0.2) |
//! | `mock`, `log-tracing`, `inspect` | no | the fork's features of the same name |
//!
//! Without `alloc`, only the component traits, [`LoadLimits`], and [`StaticKeyPath`]
//! remain: enough to declare contexts for `no_std` targets.

#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub use cgp_config_core::*;
#[cfg(feature = "alloc")]
pub use cgp_config_value::{
    AdjoinArrays, Admerge, AppendArrays, ConfigMap, ConfigValue, CoreConfigPreset, DeepMerge,
    DefaultPreparers, DenyUnknownKeys, DeserializeError, EmptyMap, IgnoreUnknownKeys, JoinBaseWins,
    KeyedMerge, Kind, LoadDefaults, LoadDefaultsOf, MergeArraysByKey, Number, PrepareFileLayer,
    REDACTED, SerdeWithProvenance, ShallowReplace, UnknownKeyPolicy, WarnUnknownKeys, de,
    from_value, from_value_with_limits, from_value_with_unknown, map, merge, preset, schema,
};
#[cfg(feature = "interpolate")]
pub use cgp_config_value::{InterpolateOptedIn, interpolate};

/// Document formats: parsers with spans and duplicate-key rejection, and renderers.
#[cfg(feature = "alloc")]
pub use cgp_config_formats as formats;

#[cfg(feature = "clap")]
pub use cgp_config_providers::cli;
#[cfg(feature = "dotenv")]
pub use cgp_config_providers::dotenv;
#[cfg(all(feature = "std", feature = "json"))]
pub use cgp_config_providers::loader::from_json_str;
#[cfg(all(feature = "std", feature = "toml"))]
pub use cgp_config_providers::loader::from_toml_str;
#[cfg(feature = "std")]
pub use cgp_config_providers::loader::{LoadSection, from_defaults};
#[cfg(feature = "std")]
pub use cgp_config_providers::{
    bootstrap, env, file, loader, location, presets, read, sources, validate,
};

/// `#[derive(ConfigSection)]`: see the [`derive`](mod@derive) module for the attributes.
#[cfg(feature = "derive")]
pub use cgp_config_derive::ConfigSection;

#[cfg(feature = "derive")]
pub mod derive;

/// The default value tree, under its short name.
#[cfg(feature = "alloc")]
pub type Value = ConfigValue;

/// One import for applications: the CGP prelude, every component, provider, preset, and
/// built-in source marker, the value tree, the derive, and the error and report types.
pub mod prelude {
    pub use cgp_config_core::components::*;
    pub use cgp_config_core::markers::*;
    pub use cgp_config_core::providers::*;
    #[cfg(feature = "alloc")]
    pub use cgp_config_core::{
        AllOf, ConfigError, EnvSnapshot, KeyPath, LoadReport, LoadWarning, Meta, Nested,
        NoopLogger, Origin, ValidateFn, ValidateSections, ValidationErrors, ValidationIssue,
        ValidationRule,
    };
    pub use cgp_config_core::{
        ConfigMeta, ConfigMetaField, ConfigSection, LoadLimits, NoConfigMeta, StaticKeyPath,
        UseConfigMeta, config_check,
    };
    #[cfg(feature = "alloc")]
    pub use cgp_config_formats::EnabledFormats;
    #[cfg(feature = "std")]
    pub use cgp_config_providers::bootstrap::{Bootstrap, Bootstrapped};
    #[cfg(feature = "std")]
    pub use cgp_config_providers::loader::{LoadSection, MemoryLoader, StdLoader};
    #[cfg(feature = "std")]
    pub use cgp_config_providers::location::{ConfigLocation, SearchPath};
    #[cfg(feature = "std")]
    pub use cgp_config_providers::presets::*;
    #[cfg(feature = "std")]
    pub use cgp_config_providers::read::{
        ConfigFileReaderComponent, ReadFromMap, ReadStdFile, VirtualFiles,
    };
    #[cfg(feature = "std")]
    pub use cgp_config_providers::sources::CliOverrides;
    #[cfg(feature = "alloc")]
    pub use cgp_config_value::{
        AdjoinArrays, Admerge, AppendArrays, ConfigMap, ConfigValue, CoreConfigPreset, DeepMerge,
        DefaultPreparers, DenyUnknownKeys, EmptyMap, IgnoreUnknownKeys, JoinBaseWins, KeyedMerge,
        LoadDefaults, MergeArraysByKey, PrepareFileLayer, SerdeWithProvenance, ShallowReplace,
        WarnUnknownKeys,
    };
    pub use cgp_fork::prelude::*;

    #[cfg(feature = "alloc")]
    pub use crate::Value;
    #[cfg(feature = "derive")]
    pub use cgp_config_derive::ConfigSection;
}

/// Paths the derive's expansion names. Not public API.
#[cfg(feature = "derive")]
#[doc(hidden)]
pub mod __private {
    pub use cgp_config_core::{
        AggregateError, CanValidateConfig, ConfigMeta, ConfigSection, ConstDefault, FieldMerge,
        FieldMeta, NestedMeta, NestedShape, ParseKind, ReloadPolicy, ValidateSections, accumulate,
        nested,
    };
    pub use cgp_fork::prelude::HasErrorType;
}
