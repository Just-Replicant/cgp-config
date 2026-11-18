//! Component traits for every stage of the loading pipeline.
//!
//! Naming follows the fork's convention: the consumer trait is `CanX` / `HasX`, the
//! provider trait is `Xer`, the wiring key is `XerComponent`. Components that are generic
//! over a marker or config type derive `UseDelegate<Param>` dispatch.

mod collect;
mod deserialize;
#[cfg(feature = "alloc")]
mod format;
mod interpolate;
mod limits;
mod loader;
mod merge;
mod prepare;
#[cfg(feature = "alloc")]
mod profile;
mod reload;
#[cfg(feature = "alloc")]
mod report;
mod secrets;
mod source;
mod validate;
mod value;

pub use collect::{CanCollectLayers, LayerCollector, LayerCollectorComponent};
pub use deserialize::{CanDeserializeConfig, ConfigDeserializer, ConfigDeserializerComponent};
#[cfg(feature = "alloc")]
pub use format::{
    CanParseFormat, CanRenderFormat, FormatParser, FormatParserComponent, FormatRenderer,
    FormatRendererComponent,
};
pub use interpolate::{CanInterpolate, Interpolator, InterpolatorComponent};
pub use limits::{HasLoadLimits, LoadLimitsComponent, LoadLimitsGetter};
pub use loader::{CanLoadConfig, ConfigLoader, ConfigLoaderComponent};
pub use merge::{CanMergeLayers, LayerMerger, LayerMergerComponent};
pub use prepare::{CanPrepareLayer, LayerPreparer, LayerPreparerComponent};
#[cfg(feature = "alloc")]
pub use profile::HasActiveProfile;
pub use reload::{
    CanReloadConfig, CanWatchChanges, ChangeEvent, ChangeWatcher, ChangeWatcherComponent,
    ConfigReloader, ConfigReloaderComponent,
};
#[cfg(feature = "alloc")]
pub use report::{CanReportWarning, HasLoadReport, LoadReportComponent, WarningReporter};
pub use secrets::{CanResolveSecrets, SecretsResolver, SecretsResolverComponent};
pub use source::{CanLoadSource, SourceLoader, SourceLoaderComponent};
pub use validate::{CanValidateConfig, ConfigValidator, ConfigValidatorComponent};
pub use value::{
    ConfigValueOf, ConfigValueTypeProvider, ConfigValueTypeProviderComponent, EmptyConfigValue,
    EmptyConfigValueComponent, HasConfigValueType, HasEmptyConfigValue,
};
