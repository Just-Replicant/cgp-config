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
//! | `LayerPreparerComponent` | `IdentityLayer` |
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
//! File-layer preparation (`$version`, `$profiles`) replaces `IdentityLayer` with a
//! per-marker table once the file sources exist.
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
        EmptyConfigValueComponent, IdentityLayer, InterpolatorComponent, LayerCollectorComponent,
        LayerMergerComponent, LayerPreparerComponent, LoadLimitsComponent, LoadReportComponent,
        NoInterpolation, NoValidation, ReportOnLoader, SecretsResolverComponent,
    };
    pub use cgp_fork::prelude::UseType;

    pub use crate::{ConfigValue, DeepMerge, EmptyMap, SerdeWithProvenance, WarnUnknownKeys};
}

use cgp_fork::prelude::*;

#[allow(clippy::wildcard_imports)]
use self::__entries::*;

cgp_preset! {
    #[version(1)]
    CoreConfigPreset {
        ConfigValueTypeProviderComponent: UseType<ConfigValue>,
        EmptyConfigValueComponent: EmptyMap,
        LayerMergerComponent: DeepMerge,
        LayerCollectorComponent: CollectInOrder,
        LayerPreparerComponent: IdentityLayer,
        InterpolatorComponent: NoInterpolation,
        ConfigDeserializerComponent: SerdeWithProvenance<WarnUnknownKeys>,
        ConfigValidatorComponent: NoValidation,
        SecretsResolverComponent: DenySecrets,
        LoadLimitsComponent: DefaultLoadLimits,
        LoadReportComponent: ReportOnLoader,
    }
}
