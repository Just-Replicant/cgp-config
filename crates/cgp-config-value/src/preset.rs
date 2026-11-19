//! [`CoreConfigPreset`]: the wiring every loader shares.
//!
//! The wiring shared by every loader context, independent of where values come from.
//!
//! | Component | Provider |
//! |---|---|
//! | value type | [`ConfigValue`] |
//! | `EmptyConfigValueComponent` | [`EmptyMap`] |
//! | `LayerMergerComponent` | [`DeepMerge`] |
//! | `LayerCollectorComponent` | `CollectInOrder` |
//! | `LayerPreparerComponent` | `UseDelegate<`[`DefaultPreparers`]`>` (file markers → [`PrepareFileLayer`], others → `IdentityLayer`) |
//! | `RootConfigMetaComponent` | `NoConfigMeta` (no schema; a context names its type with `UseConfigMeta<AppConfig>`) |
//! | `InterpolatorComponent` | `NoInterpolation` (interpolation off) |
//! | `ConfigDeserializerComponent` | [`SerdeWithProvenance<WarnUnknownKeys>`](crate::SerdeWithProvenance) (unknown keys warn) |
//! | `ConfigValidatorComponent` | `NoValidation` |
//! | `SecretsResolverComponent` | `DenySecrets` |
//! | `LoadLimitsComponent` | `DefaultLoadLimits` |
//! | `LoadReportComponent` | `ReportOnLoader` (needs a `load_report: LoadReport` field) |
//!
//! The preset has no sources and no `ConfigLoaderComponent`: a context (or a child
//! preset such as the `TwelveFactorPreset` that arrives with the source providers)
//! adds `SourceLoaderComponent` and `ConfigLoaderComponent: LoadLayered<Product![..]>`,
//! and its error type (`ErrorTypeProviderComponent`, `ErrorRaiserComponent`).
//!
//! Preparation is a per-marker table, [`DefaultPreparers`], that lists every built-in
//! marker of `cgp_config_core::markers`. A marker without an entry is a compile error,
//! which is the point: a context that adds its own marker wires its own table, keeping the
//! built-in entries it uses:
//!
//! ```text
//! LayerPreparerComponent: UseDelegate<new AppPreparers {
//!     [Defaults, EnvVars]: IdentityLayer,
//!     ConfigFile: PrepareFileLayer,
//!     MyVaultLayer: IdentityLayer,
//! }>,
//! ```
//!
//! Only the file entries (`PrepareFileLayer`) need `HasActiveProfile` (an
//! `active_profile: String` field); contexts that use only non-file markers do not.
//!
//! Behavior changes ship as a new preset type, never as a different meaning of this one.
//! Mark the context's table `#[unchecked]` and check it with `config_check!`.
//!
//! (`cgp_preset!` accepts no doc attributes, so the preset is documented here.)

// The module `cgp_preset!` generates cannot carry docs.
#![allow(missing_docs)]

#[doc(hidden)]
pub mod __entries {
    pub use cgp_config_core::{
        CollectInOrder, ConfigDeserializerComponent, ConfigValidatorComponent,
        ConfigValueTypeProviderComponent, DefaultLoadLimits, DenySecrets,
        EmptyConfigValueComponent, InterpolatorComponent, LayerCollectorComponent,
        LayerMergerComponent, LayerPreparerComponent, LoadLimitsComponent, LoadReportComponent,
        NoConfigMeta, NoInterpolation, NoValidation, ReportOnLoader, RootConfigMetaComponent,
        SecretsResolverComponent,
    };
    pub use cgp_fork::prelude::{UseDelegate, UseType};

    pub use crate::preset::DefaultPreparers;
    pub use crate::{ConfigValue, DeepMerge, EmptyMap, SerdeWithProvenance, WarnUnknownKeys};
}

use cgp_config_core::IdentityLayer;
use cgp_config_core::markers::{
    CliArgs, ConfigDir, ConfigFile, Defaults, DotEnv, DynSources, Embedded, EnvDocument, EnvVars,
    InMemory, LocalFile, ProfileFile,
};
use cgp_fork::prelude::*;

#[allow(clippy::wildcard_imports)]
use self::__entries::*;
use crate::PrepareFileLayer;

/// The preparer of every built-in source marker: documents (config, profile, local,
/// embedded files) go through [`PrepareFileLayer`], everything else through
/// `IdentityLayer`. `ConfigDir` is `IdentityLayer` because its loader prepares each
/// `conf.d` file on its own (as a `ConfigFile`) before merging them, so `$profiles` and
/// `$version` apply per file. `CoreConfigPreset` wires it as
/// `LayerPreparerComponent: UseDelegate<DefaultPreparers>`.
pub struct DefaultPreparers;

// A dispatch table, not a context: the generated check would ask the table itself to be
// a context, so it is `#[unchecked]`; `config_check!` checks the entries per marker.
delegate_components! {
    #[unchecked]
    DefaultPreparers {
        [
            ConfigFile,
            ProfileFile,
            LocalFile,
            Embedded,
        ]: PrepareFileLayer,
        [
            Defaults,
            ConfigDir,
            EnvVars,
            EnvDocument,
            DotEnv,
            CliArgs,
            InMemory,
            DynSources,
        ]: IdentityLayer,
    }
}

cgp_preset! {
    #[version(1)]
    CoreConfigPreset {
        ConfigValueTypeProviderComponent: UseType<ConfigValue>,
        EmptyConfigValueComponent: EmptyMap,
        LayerMergerComponent: DeepMerge,
        LayerCollectorComponent: CollectInOrder,
        LayerPreparerComponent: UseDelegate<DefaultPreparers>,
        InterpolatorComponent: NoInterpolation,
        ConfigDeserializerComponent: SerdeWithProvenance<WarnUnknownKeys>,
        ConfigValidatorComponent: NoValidation,
        SecretsResolverComponent: DenySecrets,
        LoadLimitsComponent: DefaultLoadLimits,
        LoadReportComponent: ReportOnLoader,
        RootConfigMetaComponent: NoConfigMeta,
    }
}
