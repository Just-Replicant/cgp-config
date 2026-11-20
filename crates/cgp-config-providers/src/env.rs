//! The `EnvVars` layer: `{PREFIX}__A__B` variables of the env snapshot as a tree.
//!
//! Rules (design plan §6.3, "Env mapping"):
//!
//! * only names starting with `{PREFIX}__` are read (bootstrap keys `{PREFIX}_CONFIG`,
//!   `{PREFIX}_PROFILE`, `{PREFIX}_DOCUMENT` use one underscore and are never fields);
//! * the rest splits on `__` (the separator is fixed: there is no single-`_` mode, see
//!   below); each segment folds to lowercase snake case (`cgp_config_core::env_fold`:
//!   `MaxConnections`, `MAX_CONNECTIONS`, and `max-connections` all fold to
//!   `max_connections`);
//! * with a root schema, a segment matches a field's document key when the two are equal
//!   ignoring ASCII case and `-`/`_` (so `APP__MAXCONNECTIONS` reads `maxConnections`) or
//!   when their snake-case folds are equal (so `APP__MaxConnections` reads
//!   `max_connections`), or its `#[config(env = "...")]` name; fields of flattened
//!   sections answer on the parent; the segment after a map section is the (folded) map
//!   key. A segment that matches two fields is `ConfigError::AmbiguousEnv` (the derive
//!   rejects such field sets at compile time, `ConfigMeta::checked`);
//! * `#[config(env_raw = "PORT")]` reads the exact, unprefixed name; a prefixed variable
//!   for the same field wins over it;
//! * values stay **strings**; the deserializer parses them by target type (lists as JSON
//!   text, never comma-split);
//! * two variables that land on the same path, or on a path and its parent, are
//!   `ConfigError::AmbiguousEnv`;
//! * with a schema, the layer is canonical: serde keys, and flattened fields under their
//!   section's field (`APP__LOG_LEVEL` → `common.log_level`).
//!
//! **`LossyUnderscore` is not available.** The design plan (§6.3) allows a context to
//! opt into a single `_` separator resolved against the schema. This release does not
//! ship it: the separator is always [`SEPARATOR`] (`__`), and `APP_HTTP_PORT` is not
//! read. A single-underscore name would need the schema to pick a split, and the
//! double-underscore form is unambiguous without one.

use std::sync::Arc;

use cgp_config_core::markers::EnvVars;
use cgp_config_core::{
    CanReportWarning, ConfigError, ConfigMeta, EnvSnapshot, FieldMeta, HasConfigValueType,
    HasEnvPrefix, HasEnvSnapshot, HasLoadLimits, HasRootConfigMeta, KeyPath, LayerSkipped,
    NestedShape, Origin, PathSegment, SourceLoaded, SourceLoader, SourceLoaderComponent, env_fold,
};
use cgp_config_value::merge::{MergeOptions, merge};
use cgp_config_value::schema::canonicalize;
use cgp_config_value::{ConfigMap, ConfigValue, Kind};
use cgp_fork::prelude::*;

/// The separator between path segments in env names.
pub const SEPARATOR: &str = "__";

/// Nesting levels of metadata followed while mapping (recursive types).
const MAX_SECTION_DEPTH: usize = cgp_config_core::FieldChain::CAPACITY;

/// One variable to map: name, value, origin.
pub type EnvEntry<'a> = (&'a str, &'a str, Origin);

/// Builds the env layer from `entries` (already filtered to the variables that may
/// belong to it), or `None` when nothing maps.
///
/// ```
/// use cgp_config_core::EnvSnapshot;
/// use cgp_config_providers::env::env_layer;
///
/// let env = EnvSnapshot::from_iter([("APP__HTTP__PORT", "9090"), ("APP_PROFILE", "prod")]);
/// let tree = env_layer(&env, "APP", None)?.unwrap();
/// assert_eq!(tree.get_path(&"http.port".into()).unwrap().as_str(), Some("9090"));
/// assert!(tree.get("profile").is_none());
/// # Ok::<(), cgp_config_core::ConfigError>(())
/// ```
///
/// # Errors
///
/// `ConfigError::AmbiguousEnv`.
pub fn env_layer(
    env: &EnvSnapshot,
    prefix: &str,
    meta: Option<&'static ConfigMeta>,
) -> Result<Option<ConfigValue>, ConfigError> {
    let marker = format!("{prefix}{SEPARATOR}");
    let entries = env
        .with_prefix(&marker)
        .map(|(key, value)| (key, value, Origin::env(key)));
    let prefixed = map_entries(entries, &marker, meta)?;
    let raw = match meta {
        Some(meta) => raw_layer(env, meta)?,
        None => None,
    };
    let layer = match (raw, prefixed) {
        (Some(raw), Some(prefixed)) => Some(merge(raw, prefixed, &MergeOptions::DEEP)?),
        (raw, prefixed) => prefixed.or(raw),
    };
    canonical(layer, meta)
}

/// The layer in canonical form (serde keys, flattened sections under their field), like
/// every other layer, so layers merge key by key.
pub(crate) fn canonical(
    layer: Option<ConfigValue>,
    meta: Option<&'static ConfigMeta>,
) -> Result<Option<ConfigValue>, ConfigError> {
    match (layer, meta) {
        (Some(layer), Some(meta)) => canonicalize(layer, meta, &mut |_| {}).map(Some),
        (layer, _) => Ok(layer),
    }
}

/// Maps `{marker}A__B` entries (the dotenv layer uses this too).
///
/// # Errors
///
/// `ConfigError::AmbiguousEnv`.
pub fn map_entries<'a>(
    entries: impl IntoIterator<Item = EnvEntry<'a>>,
    marker: &str,
    meta: Option<&'static ConfigMeta>,
) -> Result<Option<ConfigValue>, ConfigError> {
    let mut root = ConfigMap::new();
    for (key, value, origin) in entries {
        let Some(rest) = key.strip_prefix(marker) else {
            continue;
        };
        let segments: Vec<&str> = rest.split(SEPARATOR).collect();
        if segments.iter().any(|segment| segment.is_empty()) {
            // `APP____X` or a trailing separator names no field.
            continue;
        }
        let path = map_segments(key, &segments, meta)?;
        insert(
            &mut root,
            key,
            &path,
            ConfigValue::from(value).with_origin(origin),
        )?;
    }
    Ok((!root.is_empty()).then(|| ConfigValue::from(root)))
}

/// The document path of the segments of `key`.
fn map_segments(
    key: &str,
    segments: &[&str],
    meta: Option<&'static ConfigMeta>,
) -> Result<Vec<Arc<str>>, ConfigError> {
    let mut path: Vec<Arc<str>> = Vec::with_capacity(segments.len());
    let mut section = meta;
    let mut index = 0;
    while index < segments.len() {
        let segment = segments[index];
        index += 1;
        let Some(current) = section.filter(|_| path.len() < MAX_SECTION_DEPTH) else {
            path.push(fold(segment));
            continue;
        };
        let mut matches = Vec::new();
        visible_fields(current, &mut matches, 0);
        matches.retain(|field| field_matches(field, segment));
        matches.dedup_by(|a, b| a.doc_key == b.doc_key);
        match matches.as_slice() {
            [] => {
                path.push(fold(segment));
                section = None;
            }
            [field] => {
                path.push(Arc::from(field.doc_key));
                section = match field.nested {
                    Some(nested) if nested.shape == NestedShape::Single => Some(nested.section()),
                    Some(nested) if nested.shape == NestedShape::Map => {
                        if let Some(name) = segments.get(index) {
                            path.push(fold(name));
                            index += 1;
                        }
                        Some(nested.section())
                    }
                    _ => None,
                };
            }
            several => {
                let parent = key_path(&path);
                return Err(ConfigError::AmbiguousEnv {
                    key: key.into(),
                    candidates: several
                        .iter()
                        .map(|field| parent.join(field.doc_key))
                        .collect(),
                });
            }
        }
    }
    Ok(path)
}

/// The fields that answer at the level of `meta`: its own and those of flattened
/// sections.
fn visible_fields(meta: &'static ConfigMeta, out: &mut Vec<&'static FieldMeta>, depth: usize) {
    if depth >= MAX_SECTION_DEPTH {
        return;
    }
    for field in meta.fields {
        if field.flatten {
            if let Some(section) = field.single_section() {
                visible_fields(section, out, depth + 1);
            }
        } else {
            out.push(field);
        }
    }
}

fn field_matches(field: &FieldMeta, segment: &str) -> bool {
    match field.env {
        Some(env) => env.eq_ignore_ascii_case(segment),
        None => env_fold::matches(field.doc_key, segment),
    }
}

fn key_path(segments: &[Arc<str>]) -> KeyPath {
    KeyPath::from_segments(
        segments
            .iter()
            .map(|segment| PathSegment::Key(segment.clone())),
    )
}

/// Lowercase snake case of an env segment (`MaxConnections` → `max_connections`).
fn fold(segment: &str) -> Arc<str> {
    Arc::from(env_fold::fold(segment))
}

fn insert(
    root: &mut ConfigMap,
    key: &str,
    path: &[Arc<str>],
    value: ConfigValue,
) -> Result<(), ConfigError> {
    // The variable lands on a value another variable already set (`at` segments deep).
    let ambiguous = |at: usize| {
        let mut candidates = vec![key_path(&path[..at]), key_path(path)];
        candidates.dedup();
        ConfigError::AmbiguousEnv {
            key: key.into(),
            candidates,
        }
    };
    let (last, parents) = path.split_last().expect("at least one segment");
    let mut map = root;
    for (depth, segment) in parents.iter().enumerate() {
        if !map.contains_key(segment) {
            let origin = value.meta().origin.clone();
            map.insert(
                segment.clone(),
                ConfigValue::empty_map().with_origin(origin),
            );
        }
        let child = map.get_mut(segment).expect("present");
        if !child.is_map() {
            return Err(ambiguous(depth + 1));
        }
        let Kind::Map(next) = child.kind_mut() else {
            unreachable!("checked to be a map");
        };
        map = next;
    }
    if map.contains_key(last) {
        return Err(ambiguous(path.len()));
    }
    map.insert(last.clone(), value);
    Ok(())
}

/// `#[config(env_raw = "NAME")]` fields, read from their exact names.
fn raw_layer(
    env: &EnvSnapshot,
    meta: &'static ConfigMeta,
) -> Result<Option<ConfigValue>, ConfigError> {
    let mut found = Vec::new();
    collect_raw(meta, &mut Vec::new(), &mut found, 0);
    let mut root = ConfigMap::new();
    for (name, path) in found {
        if let Some(value) = env.get(name) {
            insert(
                &mut root,
                name,
                &path,
                ConfigValue::from(value).with_origin(Origin::env(name)),
            )?;
        }
    }
    Ok((!root.is_empty()).then(|| ConfigValue::from(root)))
}

fn collect_raw(
    meta: &'static ConfigMeta,
    path: &mut Vec<Arc<str>>,
    out: &mut Vec<(&'static str, Vec<Arc<str>>)>,
    depth: usize,
) {
    if depth >= MAX_SECTION_DEPTH {
        return;
    }
    for field in meta.fields {
        if field.flatten {
            if let Some(section) = field.single_section() {
                collect_raw(section, path, out, depth + 1);
            }
            continue;
        }
        path.push(Arc::from(field.doc_key));
        if let Some(name) = field.env_raw {
            out.push((name, path.clone()));
        }
        if let Some(section) = field.single_section() {
            collect_raw(section, path, out, depth + 1);
        }
        path.pop();
    }
}

/// Loads `EnvVars` from the env snapshot (`HasEnvSnapshot`) under the prefix
/// (`HasEnvPrefix`), mapped with the root schema when there is one.
pub struct LoadEnvVars;

#[cgp_impl(LoadEnvVars)]
impl SourceLoader<EnvVars>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanRaiseError<ConfigError>
        + HasEnvSnapshot
        + HasEnvPrefix
        + HasRootConfigMeta
        + HasLoadLimits
        + CanReportWarning
        + CanLog<SourceLoaded>
        + CanLog<LayerSkipped>,
{
    fn load_source(
        &self,
        _source: PhantomData<EnvVars>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        let prefix = self.env_prefix();
        let layer = env_layer(self.env_snapshot(), prefix, self.root_config_meta())
            .map_err(Self::raise_error)?;
        let origin = Origin::env(format!("{prefix}{SEPARATOR}*"));
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

#[cfg(test)]
mod tests;
