//! [`PrepareFileLayer`]: the preparer of file-backed layers.

use alloc::string::String;
use alloc::sync::Arc;

use cgp_config_core::{
    CanReportWarning, ConfigError, HasActiveProfile, HasConfigValueType, HasLoadLimits, KeyPath,
    LayerPreparer, LayerPreparerComponent, LoadLimits, LoadWarning, reserved,
};
use cgp_fork::prelude::*;

use crate::merge::{MergeOptions, merge};
use crate::{ConfigValue, Kind};

/// Prepares a layer read from a document (a config, profile, local, `conf.d`, or embedded
/// file) before it merges. Wired for the file markers in
/// [`DefaultPreparers`](crate::preset::DefaultPreparers).
///
/// 1. **`$version`** is recorded and removed: `LoadWarning::DocumentVersion` when present
///    (a non-negative integer, else `ConfigError::Type`), `LoadWarning::AssumedVersion`
///    when absent (the document is version 1). From 0.2 the migration chain runs here.
/// 2. **`$include`** is removed with `LoadWarning::IgnoredInclude` (includes load from 0.2).
/// 3. **`$profiles`** is expanded and removed: `$profiles.default`, then
///    `$profiles.{active}`, deep-merge onto the sibling keys. Nodes from a profile table
///    record the profile name in `Meta::profile`. A profile entry that is not a table is
///    `ConfigError::Type`.
///
/// The layer's root keeps its own metadata. A layer without reserved keys is returned
/// as the same allocation (after the `AssumedVersion` warning).
///
/// Only this preparer needs `Self: HasActiveProfile`; `CanPrepareLayer` itself does not
/// require it, so `no_std` contexts and contexts without file layers are unaffected
/// (ADR-001).
///
/// The "active profile is not defined anywhere" warning (`LoadWarning::MissingProfile`)
/// needs to know whether a profile *file* exists too, so it is emitted by the source
/// providers' profile loader, not here.
pub struct PrepareFileLayer;

#[cgp_impl(PrepareFileLayer)]
impl<Source> LayerPreparer<Source>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanRaiseError<ConfigError>
        + HasActiveProfile
        + CanReportWarning
        + HasLoadLimits,
{
    fn prepare_layer(
        &self,
        _source: PhantomData<Source>,
        layer: Self::ConfigValue,
    ) -> Result<Self::ConfigValue, Self::Error> {
        let limits = self.load_limits();
        prepare_file_layer(layer, self.active_profile(), &limits, &mut |warning| {
            self.report_warning(warning);
        })
        .map_err(Self::raise_error)
    }
}

/// The body of [`PrepareFileLayer`], independent of the context.
fn prepare_file_layer(
    layer: ConfigValue,
    active_profile: &str,
    limits: &LoadLimits,
    report: &mut dyn FnMut(LoadWarning),
) -> Result<ConfigValue, ConfigError> {
    let Some(map) = layer.as_map() else {
        return Ok(layer);
    };
    let origin = layer.meta().origin.clone();
    if !reserved::ALL.iter().any(|key| map.contains_key(key)) {
        report(LoadWarning::AssumedVersion { origin });
        return Ok(layer);
    }

    let mut layer = layer;
    let root_meta = layer.meta().clone();
    let map = layer.as_map_mut().expect("checked above");
    let version = map.remove(reserved::VERSION);
    let include = map.remove(reserved::INCLUDE);
    let profiles = map.remove(reserved::PROFILES);

    match version {
        Some(version) => report(LoadWarning::DocumentVersion {
            origin: origin.clone(),
            version: schema_version(&version)?,
        }),
        None => report(LoadWarning::AssumedVersion {
            origin: origin.clone(),
        }),
    }
    if include.is_some() {
        report(LoadWarning::IgnoredInclude { origin });
    }
    let Some(profiles) = profiles else {
        return Ok(layer);
    };
    let Some(tables) = profiles.as_map() else {
        return Err(type_error(
            KeyPath::root().join(reserved::PROFILES),
            "expected a table of profile tables",
            &profiles,
        ));
    };

    let options = MergeOptions::DEEP.with_limits(limits);
    let mut names = ["default", active_profile]
        .into_iter()
        .collect::<alloc::vec::Vec<_>>();
    names.dedup();
    for name in names {
        let Some(table) = tables.get(name) else {
            continue;
        };
        if !table.is_map() {
            return Err(type_error(
                KeyPath::root().join(reserved::PROFILES).join(name),
                "expected a profile table",
                table,
            ));
        }
        let mut overlay = table.clone();
        mark_profile(&mut overlay, &Arc::from(name));
        layer = merge(layer, overlay, &options)?;
    }
    Ok(layer.with_meta(root_meta))
}

/// `$version` as a `u32`.
fn schema_version(value: &ConfigValue) -> Result<u32, ConfigError> {
    value
        .as_u64()
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| {
            type_error(
                KeyPath::root().join(reserved::VERSION),
                "expected a schema version (a non-negative integer)",
                value,
            )
        })
}

fn type_error(path: KeyPath, expected: &str, found: &ConfigValue) -> ConfigError {
    let mut message = String::from(expected);
    message.push_str(", found ");
    message.push_str(found.kind_name());
    ConfigError::type_error(path, message, Some(found.meta().clone()))
}

/// Records `profile` on every node of `value`.
fn mark_profile(value: &mut ConfigValue, profile: &Arc<str>) {
    value.meta_mut().profile = Some(profile.clone());
    match value.kind_mut() {
        Kind::Array(elements) => elements.iter_mut().for_each(|e| mark_profile(e, profile)),
        Kind::Map(map) => map.values_mut().for_each(|v| mark_profile(v, profile)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use cgp_config_core::{LoadLimits, LoadWarning, Origin};

    use super::prepare_file_layer;
    use crate::ConfigValue;

    fn file(entries: ConfigValue) -> ConfigValue {
        let mut layer = entries;
        layer.stamp_origin(&Origin::file("app.toml"));
        layer
    }

    fn prepare(
        layer: ConfigValue,
        profile: &str,
    ) -> (
        Result<ConfigValue, cgp_config_core::ConfigError>,
        Vec<LoadWarning>,
    ) {
        let mut warnings = Vec::new();
        let result = prepare_file_layer(layer, profile, &LoadLimits::DEFAULT, &mut |w| {
            warnings.push(w);
        });
        (result, warnings)
    }

    #[test]
    fn a_plain_document_is_returned_as_is_with_an_assumed_version() {
        let layer = file(ConfigValue::map([("port", 1.into())]));
        let (result, warnings) = prepare(layer.clone(), "default");
        assert!(ConfigValue::ptr_eq(&result.unwrap(), &layer));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code(), "config::warn::assumed_version");
    }

    #[test]
    fn non_map_layers_pass_through() {
        let layer = ConfigValue::from(1);
        let (result, warnings) = prepare(layer.clone(), "default");
        assert!(ConfigValue::ptr_eq(&result.unwrap(), &layer));
        assert!(warnings.is_empty());
    }
}
