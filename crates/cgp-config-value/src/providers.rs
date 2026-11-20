//! Providers for the default value tree: the empty document, the merge policies, and the
//! serde deserializer with an unknown-key policy.

use alloc::vec::Vec;

use cgp_config_core::cgp::core::field::traits::StaticString;
use cgp_config_core::{
    CanReportWarning, ConfigDeserializer, ConfigDeserializerComponent, ConfigError,
    EmptyConfigValue, EmptyConfigValueComponent, HasConfigValueType, HasLoadLimits,
    HasRootConfigMeta, LayerMerger, LayerMergerComponent, LoadWarning,
};
use cgp_fork::prelude::*;
use serde::de::DeserializeOwned;

use crate::ConfigValue;
use crate::de::{UnknownKey, from_value_with_limits};
use crate::merge::{ArrayMerge, MergeDepth, MergeOptions, Precedence, merge};
use crate::schema::{denied_unknown, merge_with_schema, normalize_lenient, split_denied};

/// The empty map, as the start of the layer fold. The preset provider.
pub struct EmptyMap;

#[cgp_impl(EmptyMap)]
impl EmptyConfigValue
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>,
{
    fn empty_config_value(&self) -> Self::ConfigValue {
        ConfigValue::empty_map()
    }
}

/// Merges with `options` (recursion depth and provenance bounded by the context's limits,
/// so a deep tree fails with `LimitExceeded` instead of overflowing the stack), then checks
/// the merged tree against `max_depth` and `max_keys`.
fn merge_checked<Context>(
    context: &Context,
    base: ConfigValue,
    overlay: ConfigValue,
    options: MergeOptions,
) -> Result<ConfigValue, Context::Error>
where
    Context: CanRaiseError<ConfigError> + HasLoadLimits,
{
    let limits = context.load_limits();
    let options = options.with_limits(&limits);
    let merged = merge(base, overlay, &options).map_err(Context::raise_error)?;
    merged.check_limits(&limits).map_err(Context::raise_error)?;
    Ok(merged)
}

macro_rules! merge_provider {
    ($(#[$doc:meta])* $name:ident => $options:expr) => {
        $(#[$doc])*
        pub struct $name;

        #[cgp_impl($name)]
        impl LayerMerger
        where
            Self: HasConfigValueType<ConfigValue = ConfigValue> + CanRaiseError<ConfigError> + HasLoadLimits,
        {
            fn merge_layers(
                &self,
                base: Self::ConfigValue,
                overlay: Self::ConfigValue,
            ) -> Result<Self::ConfigValue, Self::Error> {
                merge_checked(self, base, overlay, $options)
            }
        }
    };
}

merge_provider! {
    /// Maps merge recursively; scalars and arrays from the later layer replace. The preset
    /// provider. Shape changes are `ConfigError::MergeConflict`.
    DeepMerge => MergeOptions::DEEP
}

merge_provider! {
    /// Root keys merge; the value under each root key is replaced whole by the later
    /// layer.
    ShallowReplace => MergeOptions::DEEP.with_depth(MergeDepth::Shallow)
}

merge_provider! {
    /// Like [`DeepMerge`], but two arrays at the same path concatenate (base, then
    /// overlay). Figment calls this *admerge*.
    AppendArrays => MergeOptions::DEEP.with_arrays(ArrayMerge::Append)
}

merge_provider! {
    /// Maps merge recursively; where both layers have a value, the **earlier** one wins
    /// (figment's *join*). Useful for "fill in what is missing" layers.
    JoinBaseWins => MergeOptions::DEEP.with_precedence(Precedence::BaseWins)
}

merge_provider! {
    /// [`JoinBaseWins`] with array concatenation (figment's *adjoin*).
    AdjoinArrays => MergeOptions::DEEP
        .with_precedence(Precedence::BaseWins)
        .with_arrays(ArrayMerge::Append)
}

/// Like [`DeepMerge`], but each field's `#[config(merge = "replace" | "append" | "deep")]`
/// policy (from the context's root schema, `HasRootConfigMeta`) applies at that field.
/// Without a root schema it is [`DeepMerge`].
pub struct KeyedMerge;

#[cgp_impl(KeyedMerge)]
impl LayerMerger
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanRaiseError<ConfigError>
        + HasLoadLimits
        + HasRootConfigMeta,
{
    fn merge_layers(
        &self,
        base: Self::ConfigValue,
        overlay: Self::ConfigValue,
    ) -> Result<Self::ConfigValue, Self::Error> {
        let Some(meta) = self.root_config_meta() else {
            return merge_checked(self, base, overlay, MergeOptions::DEEP);
        };
        let limits = self.load_limits();
        let options = MergeOptions::DEEP.with_limits(&limits);
        let merged = merge_with_schema(base, overlay, meta, &options).map_err(Self::raise_error)?;
        merged.check_limits(&limits).map_err(Self::raise_error)?;
        Ok(merged)
    }
}

/// Figment's name for [`AppendArrays`].
pub type Admerge = AppendArrays;

/// Like [`DeepMerge`], but arrays of maps are matched on the key named by `Key`
/// (`MergeArraysByKey<Symbol!("name")>`): matching elements merge recursively, others are
/// appended. Order: base order, then elements only in the overlay. A key repeated inside
/// one array is `ConfigError::DuplicateKey`.
pub struct MergeArraysByKey<Key>(PhantomData<Key>);

#[cgp_impl(MergeArraysByKey<Key>)]
impl<Key> LayerMerger
where
    Self:
        HasConfigValueType<ConfigValue = ConfigValue> + CanRaiseError<ConfigError> + HasLoadLimits,
    Key: StaticString,
{
    fn merge_layers(
        &self,
        base: Self::ConfigValue,
        overlay: Self::ConfigValue,
    ) -> Result<Self::ConfigValue, Self::Error> {
        merge_checked(
            self,
            base,
            overlay,
            MergeOptions::DEEP.with_arrays(ArrayMerge::ByKey(Key::VALUE)),
        )
    }
}

/// What to do with keys the config struct does not declare.
pub trait UnknownKeyPolicy<Context> {
    /// Handles the unknown keys found by one deserialization.
    ///
    /// # Errors
    ///
    /// When the policy rejects unknown keys.
    fn handle(context: &Context, unknown: Vec<UnknownKey>) -> Result<(), ConfigError>;
}

/// Records each unknown key as `LoadWarning::UnknownKey` (with a "did you mean"
/// suggestion). The preset policy.
pub struct WarnUnknownKeys;

impl<Context: CanReportWarning> UnknownKeyPolicy<Context> for WarnUnknownKeys {
    fn handle(context: &Context, unknown: Vec<UnknownKey>) -> Result<(), ConfigError> {
        for key in unknown {
            context.report_warning(LoadWarning::UnknownKey {
                path: key.path,
                suggestion: key.suggestion,
                meta: Some(key.meta),
            });
        }
        Ok(())
    }
}

/// Fails the load with one `ConfigError::Unknown` that lists **every** unknown key, in
/// document order, each with its origin and "did you mean" suggestion. Like validation,
/// it aggregates, so an operator fixes all typos in one run.
pub struct DenyUnknownKeys;

impl<Context> UnknownKeyPolicy<Context> for DenyUnknownKeys {
    fn handle(_context: &Context, unknown: Vec<UnknownKey>) -> Result<(), ConfigError> {
        ConfigError::unknown_keys(unknown).map_or(Ok(()), Err)
    }
}

/// Ignores unknown keys silently, like plain serde.
pub struct IgnoreUnknownKeys;

impl<Context> UnknownKeyPolicy<Context> for IgnoreUnknownKeys {
    fn handle(_context: &Context, _unknown: Vec<UnknownKey>) -> Result<(), ConfigError> {
        Ok(())
    }
}

/// Deserializes with the provenance-keeping, type-directed deserializer of this crate
/// (see [`crate::de`]) and applies the unknown-key `Policy`. The preset provider is
/// `SerdeWithProvenance<WarnUnknownKeys>`.
///
/// When the context names a root schema (`HasRootConfigMeta`, wired by
/// `UseConfigMeta<AppConfig>`), the tree is first normalized against it (see
/// [`crate::schema`]: flatten, aliases, mounts, durations, byte sizes, secrets, and every
/// missing required field at once), and unknown keys inside a `deny_unknown` section fail
/// the load whatever `Policy` says. Those unknown keys are reported together with the
/// normalization or deserialization errors of the same load (one `ConfigError::Validation`
/// holding them all), not only once everything else is fixed. With `NoConfigMeta` (the preset default) the tree goes
/// to serde as it is.
///
/// Sequences written as JSON text (env and CLI arrays) are parsed under the context's
/// [`HasLoadLimits`], so they cannot exceed `max_depth` or `max_keys`.
pub struct SerdeWithProvenance<Policy>(PhantomData<Policy>);

#[cgp_impl(SerdeWithProvenance<Policy>)]
impl<Policy, Config> ConfigDeserializer<Config>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanRaiseError<ConfigError>
        + HasLoadLimits
        + HasRootConfigMeta
        + CanReportWarning,
    Config: DeserializeOwned,
    Policy: UnknownKeyPolicy<Self>,
{
    fn deserialize_config(&self, value: Self::ConfigValue) -> Result<Config, Self::Error> {
        let meta = self.root_config_meta();
        let (value, normalize_error) = match meta {
            Some(meta) => {
                normalize_lenient(value, meta, &mut |warning| self.report_warning(warning))
            }
            None => (value, None),
        };
        // When normalization or serde fails, the unknown keys of `deny_unknown` sections
        // (found from the schema) join that error, so one run reports every problem.
        let with_denied = |error: ConfigError| match meta {
            Some(meta) => match ConfigError::unknown_keys(denied_unknown(&value, meta)) {
                Some(unknown) => cgp_config_core::AggregateError::aggregate(error, unknown),
                None => error,
            },
            None => error,
        };
        if let Some(error) = normalize_error {
            return Err(Self::raise_error(with_denied(error)));
        }
        let (config, unknown) = from_value_with_limits(&value, &self.load_limits())
            .map_err(|error| Self::raise_error(with_denied(error)))?;
        let unknown = match meta {
            Some(meta) => {
                let (denied, rest) = split_denied(meta, unknown);
                if let Some(error) = ConfigError::unknown_keys(denied) {
                    return Err(Self::raise_error(error));
                }
                rest
            }
            None => unknown,
        };
        Policy::handle(self, unknown).map_err(Self::raise_error)?;
        Ok(config)
    }
}
