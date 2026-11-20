//! Fallible, provenance-recording merge of two trees.
//!
//! [`merge`] is the engine behind every merge provider (`DeepMerge`, `ShallowReplace`,
//! `AppendArrays`, `JoinBaseWins`, `AdjoinArrays`, `MergeArraysByKey`). Rules:
//!
//! * **Maps** merge key by key (recursively for [`MergeDepth::Deep`]). Keys only in the
//!   base keep their position; keys only in the overlay are appended in overlay order.
//! * **Scalars** replace each other, whatever their kind: an env string `"9090"` replaces
//!   a file integer `8080`, and the deserializer parses it by the target type.
//! * **Null** is a value. A null overlay stores null; it does not delete the lower key.
//! * **Arrays** replace, append, or merge by key, per [`ArrayMerge`].
//! * A **string over an array** replaces it: env and CLI write sequences as JSON text
//!   (`APP__HOSTS='["a","b"]'`), which the deserializer parses for a sequence target.
//! * Every other **shape change** (a map over a scalar, a scalar or array over a map, an
//!   array over a scalar) is [`ConfigError::MergeConflict`] with both origins.
//!
//! The winning node records the node it replaced in its provenance chain
//! (`Meta::shadow`), capped at [`MergeOptions::max_provenance`]. This holds for containers
//! too: when two non-empty maps (or two arrays under [`ArrayMerge::Append`] or
//! [`ArrayMerge::ByKey`]) merge, the result carries the winning side's origin and records
//! the other side, so the root of a loaded document names the file it came from.
//!
//! An **empty map is the identity** on either side: merging with it returns the other side
//! unchanged, data, `Meta`, and allocation (`ConfigValue::ptr_eq` holds). Unchanged subtrees
//! are moved or shared, never deep-copied.
//!
//! An **index patch** (a map flagged `Meta::index_patch`, keys are decimal indices; the
//! CLI layer writes `--set servers[1].host=b` this way) is applied to the array below it
//! element by element: index `i` merges into element `i`, the index one past the end
//! appends, and an index further out is `ConfigError::Type` (an override never invents
//! the elements in between). With nothing below it, the indices must run from `0`
//! without gaps. Earlier elements are never dropped and nothing is padded with nulls.
//! The result of [`merge`] never contains an index patch.
//!
//! Recursion is bounded by [`MergeOptions::max_depth`]: the merge stops with
//! `ConfigError::LimitExceeded` before it descends past that depth, so a hostile or
//! accidental deep tree cannot overflow the stack.

use alloc::vec::Vec;

use cgp_config_core::{ConfigError, KeyPath, LoadLimits, Meta};

use crate::{ConfigMap, ConfigValue, Kind};

/// Which side wins where both layers have a scalar (or a non-merging container).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Precedence {
    /// The later layer wins (`DeepMerge`). The default.
    OverlayWins,
    /// The earlier layer wins where both are present (figment's *join*).
    BaseWins,
}

/// How two arrays at the same path combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArrayMerge {
    /// The winning array replaces the other. The default.
    Replace,
    /// Base elements, then overlay elements.
    Append,
    /// Arrays of maps are matched on the named key: matching elements merge, others
    /// append. Duplicate keys inside one array are [`ConfigError::DuplicateKey`].
    ByKey(&'static str),
}

/// How far map merging recurses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeDepth {
    /// Maps merge at every level. The default.
    Deep,
    /// Root keys merge; the value under each root key is replaced as a whole.
    Shallow,
}

/// Parameters of [`merge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct MergeOptions {
    /// Which side wins.
    pub precedence: Precedence,
    /// How arrays combine.
    pub arrays: ArrayMerge,
    /// How far maps merge.
    pub depth: MergeDepth,
    /// Most shadowed origins kept per node.
    pub max_provenance: usize,
    /// Deepest node the merge descends to (the root is depth 0). Deeper recursion is
    /// `ConfigError::LimitExceeded { what: "max_depth" }`.
    pub max_depth: usize,
}

impl MergeOptions {
    /// Deep merge, overlay wins, arrays replace (`DeepMerge`).
    pub const DEEP: Self = Self {
        precedence: Precedence::OverlayWins,
        arrays: ArrayMerge::Replace,
        depth: MergeDepth::Deep,
        max_provenance: LoadLimits::DEFAULT.max_provenance,
        max_depth: LoadLimits::DEFAULT.max_depth,
    };

    /// Replaces [`precedence`](Self::precedence).
    #[must_use]
    pub const fn with_precedence(mut self, precedence: Precedence) -> Self {
        self.precedence = precedence;
        self
    }

    /// Replaces [`arrays`](Self::arrays).
    #[must_use]
    pub const fn with_arrays(mut self, arrays: ArrayMerge) -> Self {
        self.arrays = arrays;
        self
    }

    /// Replaces [`depth`](Self::depth).
    #[must_use]
    pub const fn with_depth(mut self, depth: MergeDepth) -> Self {
        self.depth = depth;
        self
    }

    /// Replaces [`max_provenance`](Self::max_provenance).
    #[must_use]
    pub const fn with_max_provenance(mut self, max_provenance: usize) -> Self {
        self.max_provenance = max_provenance;
        self
    }

    /// Replaces [`max_depth`](Self::max_depth).
    #[must_use]
    pub const fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// Takes `max_depth` and `max_provenance` from `limits`.
    #[must_use]
    pub const fn with_limits(self, limits: &LoadLimits) -> Self {
        self.with_max_depth(limits.max_depth)
            .with_max_provenance(limits.max_provenance)
    }
}

impl Default for MergeOptions {
    fn default() -> Self {
        Self::DEEP
    }
}

/// Merges `overlay` onto `base`.
///
/// ```
/// use cgp_config_value::{ConfigValue, merge::{merge, MergeOptions}};
///
/// let base = ConfigValue::map([("http", ConfigValue::map([("port", 8080.into()), ("host", "0.0.0.0".into())]))]);
/// let overlay = ConfigValue::map([("http", ConfigValue::map([("port", "9090".into())]))]);
/// let merged = merge(base, overlay, &MergeOptions::DEEP)?;
/// assert_eq!(merged.get_path(&"http.port".into()).and_then(ConfigValue::as_str), Some("9090"));
/// assert_eq!(merged.get_path(&"http.host".into()).and_then(ConfigValue::as_str), Some("0.0.0.0"));
///
/// let conflict = merge(merged, ConfigValue::map([("http", 1.into())]), &MergeOptions::DEEP);
/// assert_eq!(conflict.unwrap_err().code(), "config::merge_conflict");
/// # Ok::<(), cgp_config_core::ConfigError>(())
/// ```
///
/// # Errors
///
/// [`ConfigError::MergeConflict`] on a shape change, [`ConfigError::DuplicateKey`] for
/// a repeated key under [`ArrayMerge::ByKey`], and [`ConfigError::LimitExceeded`] when
/// both trees nest deeper than [`MergeOptions::max_depth`] at the same path.
pub fn merge(
    base: ConfigValue,
    overlay: ConfigValue,
    options: &MergeOptions,
) -> Result<ConfigValue, ConfigError> {
    let patches = contains_patch(&overlay, 0, options.max_depth);
    let mut path = KeyPath::root();
    let merged = merge_at(base, overlay, options, &mut path, 0)?;
    if patches {
        let mut merged = merged;
        materialize(&mut merged, &mut KeyPath::root(), 0, options.max_depth)?;
        Ok(merged)
    } else {
        Ok(merged)
    }
}

/// [`merge`] that leaves index patches in the result, to combine several patches into one
/// layer (the CLI layer is built this way). Merge the combined layer with [`merge`].
///
/// # Errors
///
/// As [`merge`].
pub fn merge_keeping_patches(
    base: ConfigValue,
    overlay: ConfigValue,
    options: &MergeOptions,
) -> Result<ConfigValue, ConfigError> {
    merge_at(base, overlay, options, &mut KeyPath::root(), 0)
}

/// Whether `value` is an index patch (see the [module docs](self)).
#[must_use]
pub fn is_index_patch(value: &ConfigValue) -> bool {
    value.meta().index_patch && value.is_map()
}

fn contains_patch(value: &ConfigValue, level: usize, max_depth: usize) -> bool {
    if level > max_depth {
        return false;
    }
    if is_index_patch(value) {
        return true;
    }
    match value.kind() {
        Kind::Map(map) => map
            .values()
            .any(|child| contains_patch(child, level + 1, max_depth)),
        Kind::Array(elements) => elements
            .iter()
            .any(|child| contains_patch(child, level + 1, max_depth)),
        _ => false,
    }
}

/// The index of a patch key.
fn patch_index(key: &str, path: &KeyPath, meta: &Meta) -> Result<usize, ConfigError> {
    key.parse::<usize>().map_err(|_| {
        ConfigError::type_error(
            path.clone(),
            alloc::format!("`{key}` is not an array index"),
            Some(meta.clone()),
        )
    })
}

/// The entries of a patch, sorted by index.
fn patch_entries(
    patch: ConfigMap,
    path: &KeyPath,
    meta: &Meta,
) -> Result<Vec<(usize, ConfigValue)>, ConfigError> {
    let mut entries = Vec::with_capacity(patch.len());
    for (key, value) in patch {
        entries.push((patch_index(&key, path, meta)?, value));
    }
    entries.sort_by_key(|(index, _)| *index);
    Ok(entries)
}

fn index_past_end(path: &KeyPath, index: usize, len: usize, meta: &Meta) -> ConfigError {
    ConfigError::type_error(
        path.join(index),
        alloc::format!(
            "array index {index} is past the end of the array ({len} element{}); an override \
             can change an element or append the next one",
            if len == 1 { "" } else { "s" }
        ),
        Some(meta.clone()),
    )
}

/// Turns every index patch left in `value` (one with nothing below it) into an array.
fn materialize(
    value: &mut ConfigValue,
    path: &mut KeyPath,
    level: usize,
    max_depth: usize,
) -> Result<(), ConfigError> {
    if level > max_depth {
        return Err(ConfigError::limit_exceeded("max_depth", max_depth));
    }
    if !contains_patch(value, level, max_depth) {
        return Ok(());
    }
    if is_index_patch(value) {
        let taken = core::mem::replace(value, ConfigValue::null());
        let (map, mut meta) = into_map(taken);
        meta.index_patch = false;
        let entries = patch_entries(map, path, &meta)?;
        let mut elements = Vec::with_capacity(entries.len());
        for (index, element) in entries {
            if index != elements.len() {
                return Err(index_past_end(path, index, elements.len(), &meta));
            }
            elements.push(element);
        }
        *value = ConfigValue::new(Kind::Array(elements), meta);
    }
    match value.kind_mut() {
        Kind::Map(map) => {
            for (key, child) in map.iter_mut() {
                path.push(key.clone());
                let result = materialize(child, path, level + 1, max_depth);
                path.pop();
                result?;
            }
        }
        Kind::Array(elements) => {
            for (index, child) in elements.iter_mut().enumerate() {
                path.push(index);
                let result = materialize(child, path, level + 1, max_depth);
                path.pop();
                result?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Applies the index patch `overlay` onto `base`.
fn apply_patch(
    base: ConfigValue,
    overlay: ConfigValue,
    options: &MergeOptions,
    path: &mut KeyPath,
    level: usize,
) -> Result<ConfigValue, ConfigError> {
    match base.kind() {
        Kind::Array(_) => {
            let (mut elements, base_meta) = into_array(base);
            let (patch, patch_meta) = into_map(overlay);
            for (index, element) in patch_entries(patch, path, &patch_meta)? {
                if index < elements.len() {
                    path.push(index);
                    let lower = core::mem::replace(&mut elements[index], ConfigValue::null());
                    let merged = merge_at(lower, element, options, path, level + 1);
                    path.pop();
                    elements[index] = merged?;
                } else if index == elements.len() {
                    elements.push(element);
                } else {
                    return Err(index_past_end(path, index, elements.len(), &patch_meta));
                }
            }
            Ok(ConfigValue::new(Kind::Array(elements), base_meta))
        }
        // Two patches (one layer built from several overrides): merge their indices.
        Kind::Map(_) if is_index_patch(&base) => merge_maps(base, overlay, options, path, level),
        // Nothing below: the patch stands for the array (checked by `materialize`).
        Kind::Null => Ok(pick(base, overlay, options)),
        _ => Err(ConfigError::MergeConflict {
            path: path.clone(),
            base: base.meta().origin.clone(),
            overlay: overlay.meta().origin.clone(),
            base_kind: base.kind_name(),
            overlay_kind: "array",
        }),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Null,
    Scalar,
    Array,
    Map,
}

fn shape(value: &ConfigValue) -> Shape {
    match value.kind() {
        Kind::Null => Shape::Null,
        Kind::Array(_) => Shape::Array,
        Kind::Map(_) => Shape::Map,
        _ => Shape::Scalar,
    }
}

/// The winning side, with the losing side recorded in its provenance.
fn pick(base: ConfigValue, overlay: ConfigValue, options: &MergeOptions) -> ConfigValue {
    let (mut winner, loser) = match options.precedence {
        Precedence::OverlayWins => (overlay, base),
        Precedence::BaseWins => (base, overlay),
    };
    winner
        .meta_mut()
        .shadow(loser.meta(), options.max_provenance);
    winner
}

/// The metadata of a merged container: the winning side's, with the other side recorded
/// in its provenance (the container analogue of [`pick`]).
fn winning_meta(base: Meta, overlay: Meta, options: &MergeOptions) -> Meta {
    let (mut winner, loser) = match options.precedence {
        Precedence::OverlayWins => (overlay, base),
        Precedence::BaseWins => (base, overlay),
    };
    winner.shadow(&loser, options.max_provenance);
    winner
}

fn is_empty_map(value: &ConfigValue) -> bool {
    value.as_map().is_some_and(ConfigMap::is_empty)
}

fn merge_at(
    base: ConfigValue,
    overlay: ConfigValue,
    options: &MergeOptions,
    path: &mut KeyPath,
    level: usize,
) -> Result<ConfigValue, ConfigError> {
    // Checked before any recursion, so the stack depth of a merge is bounded by the limit,
    // not by the input.
    if level > options.max_depth {
        return Err(ConfigError::limit_exceeded("max_depth", options.max_depth));
    }
    if is_index_patch(&overlay) {
        return apply_patch(base, overlay, options, path, level);
    }
    if options.depth == MergeDepth::Shallow && level > 0 {
        return Ok(pick(base, overlay, options));
    }
    match (shape(&base), shape(&overlay)) {
        (Shape::Map, Shape::Map) => merge_maps(base, overlay, options, path, level),
        (Shape::Array, Shape::Array) => merge_arrays(base, overlay, options, path, level),
        (Shape::Null, _) | (_, Shape::Null) | (Shape::Scalar, Shape::Scalar) => {
            Ok(pick(base, overlay, options))
        }
        (Shape::Array, Shape::Scalar) if matches!(overlay.kind(), Kind::String(_)) => {
            Ok(pick(base, overlay, options))
        }
        _ => Err(ConfigError::MergeConflict {
            path: path.clone(),
            base: base.meta().origin.clone(),
            overlay: overlay.meta().origin.clone(),
            base_kind: base.kind_name(),
            overlay_kind: overlay.kind_name(),
        }),
    }
}

fn into_map(value: ConfigValue) -> (ConfigMap, Meta) {
    match value.into_parts() {
        (Kind::Map(map), meta) => (map, meta),
        _ => unreachable!("caller checked the shape"),
    }
}

fn into_array(value: ConfigValue) -> (Vec<ConfigValue>, Meta) {
    match value.into_parts() {
        (Kind::Array(elements), meta) => (elements, meta),
        _ => unreachable!("caller checked the shape"),
    }
}

fn merge_maps(
    base: ConfigValue,
    overlay: ConfigValue,
    options: &MergeOptions,
    path: &mut KeyPath,
    level: usize,
) -> Result<ConfigValue, ConfigError> {
    // The empty map is the identity: return the other side untouched (same `Arc`).
    if is_empty_map(&overlay) {
        return Ok(base);
    }
    if is_empty_map(&base) {
        return Ok(overlay);
    }
    let (mut map, base_meta) = into_map(base);
    let (overlay_map, overlay_meta) = into_map(overlay);
    let meta = winning_meta(base_meta, overlay_meta, options);
    for (key, overlay_value) in overlay_map {
        match map.get_mut(&key) {
            Some(slot) => {
                path.push(key.clone());
                let base_value = core::mem::replace(slot, ConfigValue::null());
                let merged = merge_at(base_value, overlay_value, options, path, level + 1);
                path.pop();
                *slot = merged?;
            }
            None => {
                map.insert(key, overlay_value);
            }
        }
    }
    Ok(ConfigValue::new(Kind::Map(map), meta))
}

fn merge_arrays(
    base: ConfigValue,
    overlay: ConfigValue,
    options: &MergeOptions,
    path: &mut KeyPath,
    level: usize,
) -> Result<ConfigValue, ConfigError> {
    match options.arrays {
        ArrayMerge::Replace => Ok(pick(base, overlay, options)),
        ArrayMerge::Append => {
            let (mut elements, base_meta) = into_array(base);
            let (overlay_elements, overlay_meta) = into_array(overlay);
            elements.extend(overlay_elements);
            let meta = winning_meta(base_meta, overlay_meta, options);
            Ok(ConfigValue::new(Kind::Array(elements), meta))
        }
        ArrayMerge::ByKey(key) => merge_by_key(base, overlay, key, options, path, level),
    }
}

/// The scalar identity of an array element under `key`, if it has one.
fn element_key<'a>(element: &'a ConfigValue, key: &str) -> Option<&'a ConfigValue> {
    element
        .get(key)
        .filter(|id| !matches!(id.kind(), Kind::Map(_) | Kind::Array(_) | Kind::Null))
}

fn check_unique(
    elements: &[ConfigValue],
    key: &str,
    path: &mut KeyPath,
) -> Result<(), ConfigError> {
    for (index, element) in elements.iter().enumerate() {
        let Some(id) = element_key(element, key) else {
            continue;
        };
        let repeated = elements[..index]
            .iter()
            .any(|earlier| element_key(earlier, key) == Some(id));
        if repeated {
            let error_path = path.join(index).join(key);
            return Err(ConfigError::duplicate_key(error_path, id.meta().clone()));
        }
    }
    Ok(())
}

fn merge_by_key(
    base: ConfigValue,
    overlay: ConfigValue,
    key: &str,
    options: &MergeOptions,
    path: &mut KeyPath,
    level: usize,
) -> Result<ConfigValue, ConfigError> {
    let (mut elements, base_meta) = into_array(base);
    let (overlay_elements, overlay_meta) = into_array(overlay);
    let meta = winning_meta(base_meta, overlay_meta, options);
    check_unique(&elements, key, path)?;
    check_unique(&overlay_elements, key, path)?;
    for overlay_element in overlay_elements {
        let position = element_key(&overlay_element, key).and_then(|id| {
            elements
                .iter()
                .position(|e| element_key(e, key) == Some(id))
        });
        match position {
            Some(index) => {
                path.push(index);
                let base_element = core::mem::replace(&mut elements[index], ConfigValue::null());
                let merged = merge_at(base_element, overlay_element, options, path, level + 1);
                path.pop();
                elements[index] = merged?;
            }
            None => elements.push(overlay_element),
        }
    }
    Ok(ConfigValue::new(Kind::Array(elements), meta))
}
