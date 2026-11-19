//! File-backed sources: the main file (`ConfigFile`), its profile and local siblings,
//! `conf.d` (`ConfigDir`), and embedded documents (`Embedded`).
//!
//! | Situation | Result |
//! |---|---|
//! | optional file missing | `Ok(None)` (logged as `LayerSkipped`) |
//! | explicit (required) file missing | `ConfigError::Missing` naming the file and the searched paths |
//! | empty TOML, JSON `null` or `{}` | an empty layer |
//! | a document whose root is not a table | `ConfigError::Type` at the root |
//! | IO, encoding, or parse failure | `Err` with the file (and `line:column` for syntax) |

use std::path::{Path, PathBuf};

use cgp_config_core::markers::{ConfigDir, ConfigFile, Embedded, LocalFile, ProfileFile};
use cgp_config_core::{
    CanMergeLayers, CanParseFormat, CanPrepareLayer, CanReportWarning, ConfigError,
    HasActiveProfile, HasConfigValueType, HasLoadLimits, KeyPath, LayerSkipped, LoadWarning,
    Origin, SourceLoaded, SourceLoader, SourceLoaderComponent, reserved,
};
use cgp_config_formats::{FormatKind, parse_document};
use cgp_config_value::{ConfigValue, Kind};
use cgp_fork::prelude::*;

use crate::location::{ConfigLocation, HasConfigLocation, is_scratch_name};
use crate::read::{CanReadConfigFile, FileText, origin_of};

/// Checks a parsed document and records the read-permission warning.
///
/// # Errors
///
/// `ConfigError::Type` when the root is not a table.
pub fn finish_document(
    tree: ConfigValue,
    file: &FileText,
    report: &mut dyn FnMut(LoadWarning),
) -> Result<ConfigValue, ConfigError> {
    let tree = match tree.kind() {
        Kind::Map(_) => tree,
        Kind::Null => ConfigValue::empty_map().with_meta(tree.meta().clone()),
        other => {
            return Err(ConfigError::type_error(
                KeyPath::root(),
                format!(
                    "expected a table at the document root, found {}",
                    other.name()
                ),
                Some(tree.meta().clone()),
            ));
        }
    };
    if file.readable_by_others && contains_secret_reference(&tree) {
        report(LoadWarning::SecretWorldReadable {
            origin: file.origin.clone(),
        });
    }
    Ok(tree)
}

/// Whether the tree holds a secret reference (`{ secret = "scheme:..." }`).
#[must_use]
pub fn contains_secret_reference(value: &ConfigValue) -> bool {
    match value.kind() {
        Kind::Map(map) => {
            let is_reference = map.len() == 1
                && map
                    .get("secret")
                    .and_then(ConfigValue::as_str)
                    .is_some_and(|reference| reference.contains(':'));
            is_reference || map.values().any(contains_secret_reference)
        }
        Kind::Array(elements) => elements.iter().any(contains_secret_reference),
        _ => false,
    }
}

/// The `ConfigError::Missing` of a required file.
#[must_use]
pub fn missing_file(path: Option<&Path>, searched: &[PathBuf]) -> ConfigError {
    let mut expected = match path {
        Some(path) => format!("config file {}", path.display()),
        None => "a config file".to_owned(),
    };
    if !searched.is_empty() {
        expected.push_str(" (searched: ");
        for (index, candidate) in searched.iter().enumerate() {
            if index > 0 {
                expected.push_str(", ");
            }
            expected.push_str(&candidate.display().to_string());
        }
        expected.push(')');
    }
    ConfigError::missing(KeyPath::root(), expected)
}

/// The shared body of the file loaders: read, parse with `parse`, check, log.
fn load_path<Context>(
    context: &Context,
    path: &Path,
    required: bool,
    searched: &[PathBuf],
    parse: impl FnOnce(&Context, &Origin, &str) -> Result<ConfigValue, Context::Error>,
) -> Result<Option<ConfigValue>, Context::Error>
where
    Context: CanReadConfigFile
        + CanRaiseError<ConfigError>
        + CanReportWarning
        + CanLog<SourceLoaded>
        + CanLog<LayerSkipped>,
{
    let Some(file) = context.read_config_file(path)? else {
        if required {
            return Err(Context::raise_error(missing_file(Some(path), searched)));
        }
        context.log(LayerSkipped::new(origin_of(path)));
        return Ok(None);
    };
    let tree = parse(context, &file.origin, &file.text)?;
    let tree = finish_document(tree, &file, &mut |warning| context.report_warning(warning))
        .map_err(Context::raise_error)?;
    context.log(SourceLoaded::new(file.origin.clone(), tree.count_entries()));
    Ok(Some(tree))
}

/// Parses by the file's extension, with whichever formats are compiled in.
fn parse_by_extension<Context>(
    context: &Context,
    origin: &Origin,
    text: &str,
    path: &Path,
) -> Result<ConfigValue, Context::Error>
where
    Context: CanRaiseError<ConfigError> + HasLoadLimits,
{
    let kind = path
        .to_str()
        .and_then(FormatKind::from_path)
        .ok_or_else(|| {
            Context::raise_error(ConfigError::parse(
                origin.clone(),
                None,
                "unknown config file extension (expected .toml or .json)",
            ))
        })?;
    parse_document(kind, origin, text, &context.load_limits()).map_err(Context::raise_error)
}

/// The main file when a location is configured, or the `Missing` error / absence.
fn main_file<Context>(context: &Context) -> Result<Option<&Path>, Context::Error>
where
    Context: HasConfigLocation + CanRaiseError<ConfigError>,
{
    let location: &ConfigLocation = context.config_location();
    match &location.file {
        Some(path) => Ok(Some(path)),
        None if location.required => {
            Err(Context::raise_error(missing_file(None, &location.searched)))
        }
        None => Ok(None),
    }
}

/// Loads `ConfigFile` from the bootstrap location (`HasConfigLocation`) with the format
/// `Fmt` (`CanParseFormat<Fmt>`, for example `LoadFile<Toml>`).
pub struct LoadFile<Fmt>(PhantomData<Fmt>);

#[cgp_impl(LoadFile<Fmt>)]
impl<Fmt> SourceLoader<ConfigFile>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanParseFormat<Fmt>
        + CanReadConfigFile
        + HasConfigLocation
        + CanRaiseError<ConfigError>
        + CanReportWarning
        + CanLog<SourceLoaded>
        + CanLog<LayerSkipped>,
{
    fn load_source(
        &self,
        _source: PhantomData<ConfigFile>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        let Some(path) = main_file(self)? else {
            return Ok(None);
        };
        let location = self.config_location();
        load_path(
            self,
            path,
            location.required,
            &location.searched,
            |context, origin, text| context.parse_format(PhantomData::<Fmt>, origin, text),
        )
    }
}

/// Loads `ConfigFile` from the bootstrap location, choosing the format by extension among
/// the enabled ones (`.toml`, `.json`). The preset loader.
pub struct LoadFileAuto;

#[cgp_impl(LoadFileAuto)]
impl SourceLoader<ConfigFile>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanReadConfigFile
        + HasConfigLocation
        + HasLoadLimits
        + CanRaiseError<ConfigError>
        + CanReportWarning
        + CanLog<SourceLoaded>
        + CanLog<LayerSkipped>,
{
    fn load_source(
        &self,
        _source: PhantomData<ConfigFile>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        let Some(path) = main_file(self)? else {
            return Ok(None);
        };
        let location = self.config_location();
        load_path(
            self,
            path,
            location.required,
            &location.searched,
            |context, origin, text| parse_by_extension(context, origin, text, path),
        )
    }
}

/// Loads `ProfileFile`: `{stem}.{profile}.{ext}` beside the main file, optional.
///
/// When the active profile is not `default` and neither this file nor a
/// `$profiles.{profile}` table in the main file exists, records
/// `LoadWarning::MissingProfile`.
pub struct LoadProfileFile;

#[cgp_impl(LoadProfileFile)]
impl SourceLoader<ProfileFile>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + HasActiveProfile
        + CanReadConfigFile
        + HasConfigLocation
        + HasLoadLimits
        + CanRaiseError<ConfigError>
        + CanReportWarning
        + CanLog<SourceLoaded>
        + CanLog<LayerSkipped>,
{
    fn load_source(
        &self,
        _source: PhantomData<ProfileFile>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        let profile = self.active_profile();
        let location = self.config_location();
        let Some(path) = location.sibling(profile) else {
            if profile != "default" {
                self.report_warning(LoadWarning::MissingProfile {
                    profile: profile.into(),
                });
            }
            return Ok(None);
        };
        let layer = load_path(self, &path, false, &[], |context, origin, text| {
            parse_by_extension(context, origin, text, &path)
        })?;
        if layer.is_none() && profile != "default" && !main_file_has_profile(self, profile)? {
            self.report_warning(LoadWarning::MissingProfile {
                profile: profile.into(),
            });
        }
        Ok(layer)
    }
}

/// Whether the main file defines `$profiles.{profile}`.
fn main_file_has_profile<Context>(context: &Context, profile: &str) -> Result<bool, Context::Error>
where
    Context: CanReadConfigFile + HasConfigLocation + HasLoadLimits + CanRaiseError<ConfigError>,
{
    let Some(path) = context.config_location().file.as_deref() else {
        return Ok(false);
    };
    let Some(file) = context.read_config_file(path)? else {
        return Ok(false);
    };
    let tree = parse_by_extension(context, &file.origin, &file.text, path)?;
    Ok(tree
        .get(reserved::PROFILES)
        .is_some_and(|profiles| profiles.get(profile).is_some()))
}

/// Loads `LocalFile`: `{stem}.local.{ext}` beside the main file (developer overrides,
/// usually git-ignored), optional.
pub struct LoadLocalFile;

#[cgp_impl(LoadLocalFile)]
impl SourceLoader<LocalFile>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanReadConfigFile
        + HasConfigLocation
        + HasLoadLimits
        + CanRaiseError<ConfigError>
        + CanReportWarning
        + CanLog<SourceLoaded>
        + CanLog<LayerSkipped>,
{
    fn load_source(
        &self,
        _source: PhantomData<LocalFile>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        let Some(path) = self.config_location().sibling("local") else {
            return Ok(None);
        };
        load_path(self, &path, false, &[], |context, origin, text| {
            parse_by_extension(context, origin, text, &path)
        })
    }
}

/// Loads `ConfigDir`: every `conf.d/*.{toml,json}` beside the main file, in sorted order,
/// skipping editor scratch files. Each file is prepared as a `ConfigFile` (so `$version`
/// and `$profiles` apply per file) and merged with the context's merger. Absent when the
/// directory is missing or holds no config file.
pub struct LoadConfDir;

#[cgp_impl(LoadConfDir)]
impl SourceLoader<ConfigDir>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanReadConfigFile
        + CanPrepareLayer<ConfigFile>
        + CanMergeLayers
        + HasConfigLocation
        + HasLoadLimits
        + CanRaiseError<ConfigError>
        + CanReportWarning
        + CanLog<SourceLoaded>
        + CanLog<LayerSkipped>,
{
    fn load_source(
        &self,
        _source: PhantomData<ConfigDir>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        let Some(dir) = self.config_location().conf_dir() else {
            return Ok(None);
        };
        let Some(entries) = self.list_config_dir(&dir)? else {
            self.log(LayerSkipped::new(origin_of(&dir)));
            return Ok(None);
        };
        let mut merged: Option<ConfigValue> = None;
        for path in entries {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let known = FormatKind::from_path(name).is_some_and(FormatKind::is_enabled);
            if is_scratch_name(name) || !known {
                continue;
            }
            let Some(layer) = load_path(self, &path, false, &[], |context, origin, text| {
                parse_by_extension(context, origin, text, &path)
            })?
            else {
                continue;
            };
            let layer = self.prepare_layer(PhantomData::<ConfigFile>, layer)?;
            merged = Some(match merged {
                Some(base) => self.merge_layers(base, layer)?,
                None => layer,
            });
        }
        Ok(merged)
    }
}

/// A document compiled into the binary, for `LoadEmbedded`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedDocument {
    /// A label for messages (`defaults.toml`).
    pub name: &'static str,
    /// The text (`include_str!("defaults.toml")`).
    pub text: &'static str,
}

/// The embedded document of a context. Implemented automatically for a context with an
/// `embedded_document: EmbeddedDocument` field.
#[cgp_auto_getter]
pub trait HasEmbeddedDocument {
    /// The document.
    fn embedded_document(&self) -> &EmbeddedDocument;
}

/// Loads `Embedded` from `HasEmbeddedDocument`, parsed as `Fmt`.
pub struct LoadEmbedded<Fmt>(PhantomData<Fmt>);

#[cgp_impl(LoadEmbedded<Fmt>)]
impl<Fmt> SourceLoader<Embedded>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanParseFormat<Fmt>
        + HasEmbeddedDocument
        + CanRaiseError<ConfigError>
        + CanLog<SourceLoaded>,
{
    fn load_source(
        &self,
        _source: PhantomData<Embedded>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        let document = self.embedded_document();
        let origin = Origin::embedded(document.name);
        let tree = self.parse_format(PhantomData::<Fmt>, &origin, document.text)?;
        let file = FileText::new(Path::new(document.name), String::new());
        let tree = finish_document(tree, &file, &mut |_| {}).map_err(Self::raise_error)?;
        self.log(SourceLoaded::new(origin, tree.count_entries()));
        Ok(Some(tree))
    }
}
