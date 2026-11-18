//! Context-generic, layered, typed configuration for Rust, built on
//! [CGP](https://contextgeneric.dev/).
//!
//! A *context* (a plain struct of yours) decides at compile time where configuration
//! comes from, how layers merge, how values deserialize, and how the result is checked.
//! Every stage is a CGP component with swappable providers; [`CoreConfigPreset`] wires
//! sensible defaults for all of them.
//!
//! This release (0.1.0-alpha.1) ships the foundation: the component set, the
//! provenance-carrying [`ConfigValue`] tree, merge policies, the provenance-aware serde
//! deserializer, [`ConfigError`], and [`CoreConfigPreset`]. File, environment, and CLI
//! sources follow in later releases; until then a context supplies its own
//! [`SourceLoader`] providers, as below.
//!
//! # Example
//!
//! ```
//! use cgp_config::prelude::*;
//! use cgp_config::cgp::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
//! use cgp_config::cgp::extra::error::RaiseFrom;
//!
//! #[derive(serde::Deserialize, Debug)]
//! struct AppConfig {
//!     port: u16,
//! }
//!
//! /// A provider that serves the built-in `Defaults` marker from memory.
//! struct BuiltIn;
//!
//! #[cgp_impl(BuiltIn)]
//! impl SourceLoader<Defaults>
//! where
//!     Self: HasConfigValueType<ConfigValue = Value> + HasErrorType,
//! {
//!     fn load_source(&self, _: PhantomData<Defaults>) -> Result<Option<Value>, Self::Error> {
//!         Ok(Some(Value::map([("port", Value::from("8080"))]).with_origin(Origin::Defaults)))
//!     }
//! }
//!
//! #[derive(HasField)]
//! struct Loader {
//!     load_report: LoadReport,
//! }
//!
//! delegate_components! {
//!     #[unchecked]
//!     Loader {
//!         preset CoreConfigPreset,
//!         ErrorTypeProviderComponent: UseType<ConfigError>,
//!         ErrorRaiserComponent: RaiseFrom,
//!         SourceLoaderComponent: BuiltIn,
//!         ConfigLoaderComponent: LoadLayered<Product![Defaults]>,
//!     }
//! }
//!
//! config_check! {
//!     Loader { config: AppConfig, sources: [Defaults] }
//! }
//!
//! let loader = Loader { load_report: LoadReport::new() };
//! let config: AppConfig = loader.load_config()?;
//! assert_eq!(config.port, 8080);
//! # Ok::<(), ConfigError>(())
//! ```
//!
//! An application depends on `cgp-config` and on `cgp-fork` directly (the CGP macros
//! name `::cgp_fork` in their expansion), and imports [`prelude`]. The prelude brings
//! every component name into scope, which presets need: `preset CoreConfigPreset`
//! replays its component keys at the use site.
//!
//! # Features
//!
//! | Feature | Default | Enables |
//! |---|---|---|
//! | `std` | yes | `alloc`, and `std` in the core and value crates |
//! | `alloc` | via `std` | the value tree, merge, deserializer, errors, and `CoreConfigPreset` |
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
    Kind, MergeArraysByKey, Number, PrepareFileLayer, REDACTED, SerdeWithProvenance,
    ShallowReplace, UnknownKeyPolicy, WarnUnknownKeys, de, from_value, from_value_with_limits,
    from_value_with_unknown, map, merge, preset,
};

/// The default value tree, under its short name.
#[cfg(feature = "alloc")]
pub type Value = ConfigValue;

/// One import for applications: the CGP prelude, every component, provider, and built-in
/// source marker of this crate, the value tree, and the error and report types.
pub mod prelude {
    pub use cgp_config_core::components::*;
    pub use cgp_config_core::markers::*;
    pub use cgp_config_core::providers::*;
    #[cfg(feature = "alloc")]
    pub use cgp_config_core::{
        ConfigError, KeyPath, LoadReport, LoadWarning, Meta, Origin, ValidationErrors,
        ValidationIssue,
    };
    pub use cgp_config_core::{LoadLimits, StaticKeyPath, config_check};
    #[cfg(feature = "alloc")]
    pub use cgp_config_value::{
        AdjoinArrays, Admerge, AppendArrays, ConfigMap, ConfigValue, CoreConfigPreset, DeepMerge,
        DefaultPreparers, DenyUnknownKeys, EmptyMap, IgnoreUnknownKeys, JoinBaseWins,
        MergeArraysByKey, PrepareFileLayer, SerdeWithProvenance, ShallowReplace, WarnUnknownKeys,
    };
    pub use cgp_fork::prelude::*;

    #[cfg(feature = "alloc")]
    pub use crate::Value;
}
