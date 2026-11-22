//! The remaining sources: in-memory trees (`InMemory`), dynamic sources (`DynSources`),
//! the JSON env document (`EnvDocument`), and command-line overrides (`CliArgs`).

use std::collections::BTreeMap;

use cgp_config_core::markers::{CliArgs, DynSources, InMemory};
use cgp_config_core::{
    CanMergeLayers, CanReportWarning, ConfigError, HasConfigValueType, HasLoadLimits,
    HasRootConfigMeta, KeyPath, LayerSkipped, Origin, PathSegment, SourceLoaded, SourceLoader,
    SourceLoaderComponent,
};
use cgp_config_value::schema::canonicalize;
use cgp_config_value::{ConfigMap, ConfigValue, Kind};
use cgp_fork::prelude::*;

/// Loads `InMemory` from the context field named by `Tag`
/// (`LoadFromField<Symbol!("memory_layer")>` reads `memory_layer: ConfigValue`). An empty
/// map is absent.
pub struct LoadFromField<Tag>(PhantomData<Tag>);

#[cgp_impl(LoadFromField<Tag>)]
impl<Tag> SourceLoader<InMemory>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + HasErrorType
        + HasField<Tag, Value = ConfigValue>,
{
    fn load_source(
        &self,
        _source: PhantomData<InMemory>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        let layer = self.get_field(PhantomData::<Tag>);
        let empty = layer.as_map().is_some_and(ConfigMap::is_empty);
        Ok((!empty).then(|| layer.clone()))
    }
}

/// An object-safe source with a fixed error type: the escape hatch for sources chosen at
/// run time (plugins, remote stores). Contexts with their own value tree add a typed
/// marker instead.
pub trait DynSource: Send + Sync {
    /// The layer, or `None` when the source is absent.
    ///
    /// # Errors
    ///
    /// When the source is present but cannot be read.
    fn load(&self) -> Result<Option<ConfigValue>, ConfigError>;
}

impl<F> DynSource for F
where
    F: Fn() -> Result<Option<ConfigValue>, ConfigError> + Send + Sync,
{
    fn load(&self) -> Result<Option<ConfigValue>, ConfigError> {
        self()
    }
}

/// The dynamic sources of a context, merged in order. Implemented automatically for a
/// context with a `dyn_sources: Vec<Box<dyn DynSource>>` field.
#[cgp_auto_getter]
pub trait HasDynSources {
    /// The sources.
    fn dyn_sources(&self) -> &Vec<Box<dyn DynSource>>;
}

/// Loads `DynSources`: every source of `HasDynSources` in order, merged with the
/// context's merger. Absent when none produced a layer.
pub struct LoadDynSources;

#[cgp_impl(LoadDynSources)]
impl SourceLoader<DynSources>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanRaiseError<ConfigError>
        + CanMergeLayers
        + HasDynSources
        + HasLoadLimits,
{
    fn load_source(
        &self,
        _source: PhantomData<DynSources>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        let mut merged: Option<ConfigValue> = None;
        for source in self.dyn_sources() {
            let Some(layer) = source.load().map_err(Self::raise_error)? else {
                continue;
            };
            layer
                .check_limits(&self.load_limits())
                .map_err(Self::raise_error)?;
            merged = Some(match merged {
                Some(base) => self.merge_layers(base, layer)?,
                None => layer,
            });
        }
        Ok(merged)
    }
}

/// Command-line overrides as `path = value` pairs (`--set http.port=8080`), already
/// extracted by the application (or by the `clap` helpers). Paths are document paths
/// (`http.port`, `servers[0].host`); values are strings, parsed by target type like env
/// values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliOverrides {
    entries: BTreeMap<String, String>,
}

impl CliOverrides {
    /// No overrides.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `path = value`.
    #[must_use]
    pub fn with(mut self, path: impl Into<String>, value: impl Into<String>) -> Self {
        self.insert(path, value);
        self
    }

    /// Adds or replaces `path = value`.
    pub fn insert(&mut self, path: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(path.into(), value.into());
    }

    /// Parses `path=value`.
    ///
    /// # Errors
    ///
    /// A message when there is no `=` or the path is empty.
    pub fn insert_assignment(&mut self, assignment: &str) -> Result<(), String> {
        let (path, value) = assignment
            .split_once('=')
            .ok_or_else(|| format!("expected `key=value`, found `{assignment}`"))?;
        let path = path.trim();
        if path.is_empty() {
            return Err("expected `key=value` with a non-empty key".into());
        }
        self.insert(path, value);
        Ok(())
    }

    /// Whether there are no overrides.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The overrides, sorted by path.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// The overrides as a layer (`Origin::Cli` with `--set {path}`).
    ///
    /// # Errors
    ///
    /// `ConfigError::Parse` for an invalid path; `AmbiguousEnv`-style conflicts (a value
    /// and a child of it) are `ConfigError::MergeConflict`.
    pub fn to_layer(&self) -> Result<Option<ConfigValue>, ConfigError> {
        let mut root = ConfigValue::empty_map();
        for (path, value) in self.iter() {
            let origin = Origin::cli(format!("--set {path}"));
            let parsed = KeyPath::parse(path).map_err(|error| {
                ConfigError::parse(
                    origin.clone(),
                    None,
                    format!("invalid override path: {error}"),
                )
            })?;
            let too_large = parsed.segments().iter().any(
                |segment| matches!(segment, PathSegment::Index(index) if *index >= MAX_OVERRIDE_INDEX),
            );
            if too_large {
                return Err(ConfigError::parse(
                    origin,
                    None,
                    format!("override index must be below {MAX_OVERRIDE_INDEX}"),
                ));
            }
            let leaf = ConfigValue::from(value).with_origin(origin.clone());
            let layer = nest(&parsed, leaf, &origin);
            root = cgp_config_value::merge::merge_keeping_patches(
                root,
                layer,
                &cgp_config_value::merge::MergeOptions::DEEP,
            )?;
        }
        Ok(root
            .as_map()
            .is_some_and(|map| !map.is_empty())
            .then_some(root))
    }
}

/// The largest array index an override may name (`servers[4095]`). An index segment
/// patches that one element of the array below (see `cgp_config_value::merge`), so no
/// padding is allocated; the bound only rejects indices no real array reaches.
pub const MAX_OVERRIDE_INDEX: usize = 4096;

/// `value` placed at `path`. A key segment becomes a one-entry map; an index segment
/// becomes an *index patch* (a map keyed by the decimal index, flagged
/// `Meta::index_patch`) that the merge applies to that element of the array below,
/// leaving the other elements alone.
fn nest(path: &KeyPath, value: ConfigValue, origin: &Origin) -> ConfigValue {
    path.segments()
        .iter()
        .rev()
        .fold(value, |inner, segment| match segment {
            PathSegment::Key(key) => ConfigValue::new(
                Kind::Map([(key.clone(), inner)].into_iter().collect()),
                cgp_config_core::Meta::new(origin.clone()),
            ),
            PathSegment::Index(index) => {
                let mut meta = cgp_config_core::Meta::new(origin.clone());
                meta.index_patch = true;
                ConfigValue::new(
                    Kind::Map([(index.to_string(), inner)].into_iter().collect()),
                    meta,
                )
            }
        })
}

/// The command-line overrides of a context. Implemented automatically for a context
/// with a `cli_overrides: CliOverrides` field.
#[cgp_auto_getter]
pub trait HasCliOverrides {
    /// The overrides.
    fn cli_overrides(&self) -> &CliOverrides;
}

/// Loads `CliArgs` from `HasCliOverrides`. Never parses `argv` itself.
pub struct LoadCliOverrides;

#[cgp_impl(LoadCliOverrides)]
impl SourceLoader<CliArgs>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanRaiseError<ConfigError>
        + HasCliOverrides
        + HasRootConfigMeta
        + HasLoadLimits
        + CanReportWarning
        + CanLog<SourceLoaded>
        + CanLog<LayerSkipped>,
{
    fn load_source(
        &self,
        _source: PhantomData<CliArgs>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        let layer = self.cli_overrides().to_layer().map_err(Self::raise_error)?;
        // Overrides use document keys (aliases warn); the layer merges in canonical form.
        let layer = match (layer, self.root_config_meta()) {
            (Some(layer), Some(meta)) => Some(
                canonicalize(layer, meta, &mut |warning| self.report_warning(warning))
                    .map_err(Self::raise_error)?,
            ),
            (layer, _) => layer,
        };
        let origin = Origin::cli("--set");
        match &layer {
            Some(layer) => {
                layer
                    .check_limits(&self.load_limits())
                    .map_err(Self::raise_error)?;
                self.log(SourceLoaded::new(origin, layer.count_entries()));
            }
            None => self.log(LayerSkipped::new(origin)),
        }
        Ok(layer)
    }
}

/// The raw `{PREFIX}_DOCUMENT` text bootstrap took out of the snapshot. Implemented
/// automatically for a context with an `env_document: Option<String>` field.
#[cgp_auto_getter]
pub trait HasEnvDocument {
    /// The document text, when the variable was set.
    fn env_document(&self) -> &Option<String>;
}

#[cfg(feature = "json")]
pub use env_document::LoadEnvDocument;

#[cfg(feature = "json")]
mod env_document {
    use cgp_config_core::markers::EnvDocument;
    use cgp_config_core::{
        CanReportWarning, ConfigError, HasConfigValueType, HasEnvPrefix, HasLoadLimits,
        HasRootConfigMeta, LayerSkipped, Origin, SourceLoaded, SourceLoader, SourceLoaderComponent,
    };
    use cgp_config_value::ConfigValue;
    use cgp_config_value::schema::canonicalize;
    use cgp_fork::prelude::*;

    use super::HasEnvDocument;
    use crate::file::finish_document;
    use crate::read::FileText;

    /// Loads `EnvDocument`: `{PREFIX}_DOCUMENT` as one JSON object (feature `json`).
    /// Absent when the variable was not set. Like the CLI layer, the document is
    /// canonicalized against the root schema (aliases and mounted keys renamed, with
    /// `DeprecatedKey` warnings; flattened keys moved under their field), so it merges
    /// with the other layers by field, not by spelling.
    pub struct LoadEnvDocument;

    #[cgp_impl(LoadEnvDocument)]
    impl SourceLoader<EnvDocument>
    where
        Self: HasConfigValueType<ConfigValue = ConfigValue>
            + CanRaiseError<ConfigError>
            + HasEnvDocument
            + HasEnvPrefix
            + HasRootConfigMeta
            + HasLoadLimits
            + CanReportWarning
            + CanLog<SourceLoaded>
            + CanLog<LayerSkipped>,
    {
        fn load_source(
            &self,
            _source: PhantomData<EnvDocument>,
        ) -> Result<Option<Self::ConfigValue>, Self::Error> {
            let origin = Origin::env_document(format!("{}_DOCUMENT", self.env_prefix()));
            let Some(text) = self.env_document() else {
                self.log(LayerSkipped::new(origin));
                return Ok(None);
            };
            let tree = cgp_config_formats::parse_json(&origin, text, &self.load_limits())
                .map_err(Self::raise_error)?;
            let mut file = FileText::new(std::path::Path::new(""), String::new());
            file.origin = origin.clone();
            let tree = finish_document(tree, &file, self.root_config_meta(), &mut |w| {
                self.report_warning(w)
            })
            .map_err(Self::raise_error)?;
            let tree = match self.root_config_meta() {
                Some(meta) => canonicalize(tree, meta, &mut |w| self.report_warning(w))
                    .map_err(Self::raise_error)?,
                None => tree,
            };
            self.log(SourceLoaded::new(origin, tree.count_entries()));
            Ok(Some(tree))
        }
    }
}

#[cfg(test)]
mod tests {
    use cgp_config_value::ConfigValue;
    use cgp_config_value::merge::{MergeOptions, merge};

    use super::CliOverrides;

    fn servers_file() -> ConfigValue {
        let server = |host: &str, port: i64| {
            ConfigValue::map([
                ("host", ConfigValue::from(host)),
                ("port", ConfigValue::from(port)),
            ])
        };
        ConfigValue::map([(
            "servers",
            ConfigValue::array([server("a", 1), server("b", 2), server("c", 3)]),
        )])
    }

    #[test]
    fn index_override_patches_one_element() {
        let layer = CliOverrides::new()
            .with("servers[1].host", "x")
            .with("servers[2].port", "9")
            .to_layer()
            .unwrap()
            .unwrap();
        let merged = merge(servers_file(), layer, &MergeOptions::DEEP).unwrap();
        let host = |i: usize| {
            merged
                .get_path(&format!("servers[{i}].host").as_str().into())
                .unwrap()
                .as_str()
                .map(str::to_owned)
        };
        assert_eq!(merged.get("servers").unwrap().as_array().unwrap().len(), 3);
        assert_eq!(host(0).as_deref(), Some("a"));
        assert_eq!(host(1).as_deref(), Some("x"));
        assert_eq!(host(2).as_deref(), Some("c"));
        // Untouched fields of a patched element stay.
        assert!(
            merged
                .get_path(&"servers[1].port".into())
                .unwrap()
                .as_i64()
                .is_some()
        );
        assert_eq!(
            merged.get_path(&"servers[2].port".into()).unwrap().as_str(),
            Some("9")
        );
    }

    #[test]
    fn index_override_appends_or_rejects_gaps() {
        let append = CliOverrides::new().with("servers[3].host", "d");
        let merged = merge(
            servers_file(),
            append.to_layer().unwrap().unwrap(),
            &MergeOptions::DEEP,
        )
        .unwrap();
        assert_eq!(merged.get("servers").unwrap().as_array().unwrap().len(), 4);
        let gap = CliOverrides::new().with("servers[5].host", "d");
        let error = merge(
            servers_file(),
            gap.to_layer().unwrap().unwrap(),
            &MergeOptions::DEEP,
        )
        .unwrap_err();
        assert_eq!(error.code(), "config::type");
        assert!(error.to_string().contains("past the end"), "{error}");
        // With nothing below, indices must start at 0 without gaps.
        let fresh = CliOverrides::new()
            .with("list[0]", "a")
            .with("list[1]", "b");
        let merged = merge(
            ConfigValue::empty_map(),
            fresh.to_layer().unwrap().unwrap(),
            &MergeOptions::DEEP,
        )
        .unwrap();
        assert_eq!(merged.get("list").unwrap().as_array().unwrap().len(), 2);
        let sparse = CliOverrides::new().with("list[1]", "b");
        assert!(
            merge(
                ConfigValue::empty_map(),
                sparse.to_layer().unwrap().unwrap(),
                &MergeOptions::DEEP,
            )
            .is_err()
        );
        // A patch over a scalar is a conflict, not a silent replacement.
        let scalar = ConfigValue::map([("servers", ConfigValue::from("a,b"))]);
        let error = merge(
            scalar,
            CliOverrides::new()
                .with("servers[0]", "x")
                .to_layer()
                .unwrap()
                .unwrap(),
            &MergeOptions::DEEP,
        )
        .unwrap_err();
        assert_eq!(error.code(), "config::merge_conflict");
    }

    #[test]
    fn overrides_become_a_layer() {
        let mut overrides = CliOverrides::new().with("http.port", "9090");
        overrides.insert_assignment("servers[1].host=b").unwrap();
        overrides.insert_assignment(" name = x=y").unwrap();
        assert!(overrides.insert_assignment("novalue").is_err());
        assert!(overrides.insert_assignment("=1").is_err());
        let layer = overrides.to_layer().unwrap().unwrap();
        assert_eq!(
            layer.get_path(&"http.port".into()).unwrap().as_str(),
            Some("9090")
        );
        // `servers[1]` is an index patch for element 1 only: no null padding.
        let servers = layer.get("servers").unwrap();
        assert!(cgp_config_value::merge::is_index_patch(servers));
        assert_eq!(servers.as_map().unwrap().len(), 1);
        assert_eq!(
            servers.get("1").unwrap().get("host").unwrap().as_str(),
            Some("b")
        );
        assert_eq!(layer.get("name").unwrap().as_str(), Some(" x=y"));
        assert_eq!(
            layer
                .get_path(&"http.port".into())
                .unwrap()
                .meta()
                .origin
                .to_string(),
            "cli --set http.port"
        );
        assert!(CliOverrides::new().to_layer().unwrap().is_none());
        let huge = CliOverrides::new().with("a[999999999]", "1");
        assert_eq!(huge.to_layer().unwrap_err().code(), "config::parse");
        let conflict = CliOverrides::new().with("a", "1").with("a.b", "2");
        assert_eq!(
            conflict.to_layer().unwrap_err().code(),
            "config::merge_conflict"
        );
        let conflict = CliOverrides::new().with("a[0]", "1").with("a.b", "2");
        assert_eq!(
            conflict.to_layer().unwrap_err().code(),
            "config::merge_conflict"
        );
    }
}
