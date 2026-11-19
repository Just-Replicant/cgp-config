//! Schema passes: the tree in *document form* (what files, env, and CLI write) against the
//! [`ConfigMeta`] of the type being loaded.
//!
//! * [`defaults_tree`] builds the `Defaults` layer from const defaults;
//! * [`normalize`] turns document form into the shape serde expects, right before
//!   deserialization;
//! * [`split_denied`] applies per-section `deny_unknown` to the keys serde ignored.
//!
//! [`normalize`] does, per section and recursively through nested sections:
//!
//! 1. **flatten**: keys of a `#[config(flatten)]` section, written on the parent, move
//!    under the flattened field (a non-optional flattened field always gets a map);
//! 2. **aliases and mounts**: an alias or a mounted document key is renamed to the serde
//!    key; an alias records `LoadWarning::DeprecatedKey`, and an alias next to the current
//!    name is `ConfigError::DuplicateKey`;
//! 3. **`deprecated`** fields that are present record `LoadWarning::DeprecatedKey`;
//! 4. **`secret`** marks the subtree, so errors and dumps never echo it;
//! 5. **`parse`**: durations (`"30s"` to `{secs, nanos}`, the serde form of
//!    `core::time::Duration`; a bare number is rejected) and byte sizes (`"10MiB"` to an
//!    integer), elementwise inside arrays;
//! 6. **required fields**: every absent field marked `required` is reported.
//!
//! Every failure of the pass is reported together: one error stays as it is, several are
//! folded into one `ConfigError::Validation` (see `AggregateError`).

use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;

use cgp_config_core::units::{parse_bytes, parse_duration};
use cgp_config_core::{
    AggregateError, ConfigError, ConfigMeta, ConstDefault, FieldChain, FieldMeta, KeyPath,
    LoadWarning, NestedShape, Origin, ParseKind, PathSegment, UnknownKey, accumulate,
};

use crate::{ConfigMap, ConfigValue, Kind, Number};

/// Nesting levels of metadata followed before giving up (recursive types).
const MAX_SECTION_DEPTH: usize = FieldChain::CAPACITY;

/// The `Defaults` layer of `meta`, in canonical form: every const default at its serde
/// key, nested sections as maps (flattened sections under their own field, as
/// [`canonicalize`] leaves every other layer), stamped `Origin::Defaults`.
/// Optional sections get no defaults, so they stay `None` unless a layer sets them.
///
/// ```
/// use cgp_config_core::{ConfigMeta, ConstDefault, FieldMeta};
/// use cgp_config_value::schema::defaults_tree;
///
/// static META: ConfigMeta = ConfigMeta::new(
///     "Http",
///     &[FieldMeta::new("port", "port").with_default(ConstDefault::UInt(8080))],
/// );
/// let tree = defaults_tree(&META);
/// assert_eq!(tree.get("port").and_then(|v| v.as_u64()), Some(8080));
/// ```
#[must_use]
pub fn defaults_tree(meta: &'static ConfigMeta) -> ConfigValue {
    let mut map = ConfigMap::new();
    defaults_into(meta, &mut map, 0);
    let mut tree = ConfigValue::from(map);
    tree.stamp_origin(&Origin::Defaults);
    tree
}

fn defaults_into(meta: &'static ConfigMeta, map: &mut ConfigMap, depth: usize) {
    if depth >= MAX_SECTION_DEPTH {
        return;
    }
    for field in meta.fields {
        if let Some(default) = field.default.filter(|_| !field.flatten) {
            map.insert(field.key, const_value(default));
        } else if let Some(section) = field.single_section().filter(|_| !field.optional) {
            let mut sub = ConfigMap::new();
            defaults_into(section, &mut sub, depth + 1);
            if !sub.is_empty() {
                map.insert(field.key, ConfigValue::from(sub));
            }
        }
    }
}

/// A [`ConstDefault`] as a value.
#[must_use]
pub fn const_value(default: ConstDefault) -> ConfigValue {
    match default {
        ConstDefault::Bool(value) => value.into(),
        ConstDefault::Int(value) => Number::from_i64(value).into(),
        ConstDefault::UInt(value) => Number::from_u64(value).into(),
        ConstDefault::Float(value) => Number::from_f64(value).into(),
        ConstDefault::Str(value) => value.into(),
        ConstDefault::Array(values) => ConfigValue::array(values.iter().copied().map(const_value)),
        // `ConstDefault` is non-exhaustive for downstream crates only.
        #[allow(unreachable_patterns)]
        _ => ConfigValue::null(),
    }
}

/// Normalizes `value` (document form) against `meta`. See the [module docs](self).
///
/// # Errors
///
/// Every duration, byte-size, duplicate-alias, and missing-field failure, together.
pub fn normalize(
    mut value: ConfigValue,
    meta: &'static ConfigMeta,
    report: &mut dyn FnMut(LoadWarning),
) -> Result<ConfigValue, ConfigError> {
    let mut pass = Pass {
        report,
        errors: None,
        full: true,
    };
    pass.section(&mut value, meta, &KeyPath::root(), 0);
    pass.errors.map_or(Ok(value), Err)
}

/// Renames aliases and mounted keys and moves flattened keys, in one layer, before it is
/// merged with others. File preparation (`PrepareFileLayer`) runs this so an alias in one
/// layer and the current name in another merge by precedence instead of colliding.
/// Aliases record `LoadWarning::DeprecatedKey`; parsers, secrets, and required fields
/// wait for [`normalize`] on the merged tree.
///
/// # Errors
///
/// `ConfigError::DuplicateKey` when one layer sets a field under two names.
pub fn canonicalize(
    mut value: ConfigValue,
    meta: &'static ConfigMeta,
    report: &mut dyn FnMut(LoadWarning),
) -> Result<ConfigValue, ConfigError> {
    let mut pass = Pass {
        report,
        errors: None,
        full: false,
    };
    pass.section(&mut value, meta, &KeyPath::root(), 0);
    pass.errors.map_or(Ok(value), Err)
}

struct Pass<'a> {
    report: &'a mut dyn FnMut(LoadWarning),
    errors: Option<ConfigError>,
    /// `normalize` (everything) or `canonicalize` (names and flattening only).
    full: bool,
}

impl Pass<'_> {
    fn fail(&mut self, error: ConfigError) {
        self.errors = accumulate(self.errors.take(), error);
    }

    fn section(
        &mut self,
        value: &mut ConfigValue,
        meta: &'static ConfigMeta,
        path: &KeyPath,
        depth: usize,
    ) {
        if depth >= MAX_SECTION_DEPTH {
            return;
        }
        let origin = value.meta().origin.clone();
        let Some(map) = value.as_map_mut() else {
            // Not a table: serde reports the type error with the node's provenance.
            return;
        };
        Self::flatten(map, meta, &origin);
        self.rename(map, meta, path);
        for field in meta.fields {
            let field_path = path.join(field.key);
            let Some(child) = map.get_mut(field.key) else {
                if field.required && self.full {
                    self.fail(ConfigError::missing(field_path, field.ty));
                }
                continue;
            };
            if field.secret && self.full {
                mark_secret(child);
            }
            if let Some(kind) = field.parse.filter(|_| self.full) {
                self.parse(child, kind, &field_path);
            }
            let Some(nested) = field.nested else {
                continue;
            };
            let section = nested.section();
            match nested.shape {
                NestedShape::Single => self.section(child, section, &field_path, depth + 1),
                NestedShape::Seq => {
                    if let Kind::Array(elements) = child.kind_mut() {
                        for (index, element) in elements.iter_mut().enumerate() {
                            let element_path = field_path.join(PathSegment::Index(index));
                            if self.full {
                                fill_defaults(element, section);
                            }
                            self.section(element, section, &element_path, depth + 1);
                        }
                    }
                }
                NestedShape::Map => {
                    if let Kind::Map(entries) = child.kind_mut() {
                        let keys: Vec<Arc<str>> =
                            entries.iter_shared().map(|(key, _)| key.clone()).collect();
                        for key in keys {
                            if let Some(entry) = entries.get_mut(&key) {
                                let entry_path = field_path.join(PathSegment::Key(key.clone()));
                                if self.full {
                                    fill_defaults(entry, section);
                                }
                                self.section(entry, section, &entry_path, depth + 1);
                            }
                        }
                    }
                }
                #[allow(unreachable_patterns)]
                _ => {}
            }
        }
    }

    /// Moves the keys of flattened sections under their field.
    fn flatten(map: &mut ConfigMap, meta: &'static ConfigMeta, origin: &Origin) {
        for field in meta.fields.iter().filter(|field| field.flatten) {
            let Some(section) = field.single_section() else {
                continue;
            };
            let moved: Vec<Arc<str>> = map
                .iter_shared()
                .filter(|(key, _)| {
                    !answers_directly(meta, key) && section.resolve_doc_key(key).is_some()
                })
                .map(|(key, _)| key.clone())
                .collect();
            let existing = map.remove(field.key).filter(ConfigValue::is_map);
            if moved.is_empty() && existing.is_none() && field.optional {
                continue;
            }
            let mut sub =
                existing.unwrap_or_else(|| ConfigValue::empty_map().with_origin(origin.clone()));
            let sub_map = sub.as_map_mut().expect("a map by construction");
            for key in moved {
                if let Some(entry) = map.remove(&key) {
                    sub_map.insert(key, entry);
                }
            }
            map.insert(field.key, sub);
        }
    }

    /// Renames aliases and mounted keys to the serde key, and reports deprecations.
    fn rename(&mut self, map: &mut ConfigMap, meta: &'static ConfigMeta, path: &KeyPath) {
        for field in meta.fields.iter().filter(|field| !field.flatten) {
            let field_path = path.join(field.key);
            let mut current = if field.doc_key == field.key {
                None
            } else {
                map.remove(field.doc_key)
            };
            let in_place = field.doc_key == field.key && map.contains_key(field.key);
            if self.full
                && (in_place || current.is_some())
                && let Some(note) = field.deprecated
            {
                (self.report)(LoadWarning::DeprecatedKey {
                    path: field_path.clone(),
                    alias: field.doc_key.into(),
                    note: note.into(),
                });
            }
            for alias in field.aliases {
                let Some(found) = map.remove(alias) else {
                    continue;
                };
                // A built-in default under the current name gives way to an alias set
                // by a layer (defaults are always the lowest layer).
                let default_in_place = in_place
                    && current.is_none()
                    && map
                        .get(field.key)
                        .is_some_and(|value| matches!(value.meta().origin, Origin::Defaults));
                if default_in_place {
                    map.remove(field.key);
                } else if in_place || current.is_some() {
                    self.fail(ConfigError::duplicate_key(
                        field_path.clone(),
                        found.meta().clone(),
                    ));
                    continue;
                }
                (self.report)(LoadWarning::DeprecatedKey {
                    path: field_path.clone(),
                    alias: (*alias).into(),
                    note: field
                        .deprecated
                        .map_or_else(|| format!("renamed to `{}`", field.doc_key), Into::into),
                });
                current = Some(found);
            }
            if let Some(value) = current {
                if map.contains_key(field.key) {
                    self.fail(ConfigError::duplicate_key(field_path, value.meta().clone()));
                } else {
                    map.insert(field.key, value);
                }
            }
        }
    }

    fn parse(&mut self, value: &mut ConfigValue, kind: ParseKind, path: &KeyPath) {
        if let Kind::Array(elements) = value.kind_mut() {
            for (index, element) in elements.iter_mut().enumerate() {
                self.parse(element, kind, &path.join(PathSegment::Index(index)));
            }
            return;
        }
        if let Err(message) = parse_node(value, kind) {
            self.fail(ConfigError::type_error(
                path.clone(),
                message,
                Some(value.meta().clone()),
            ));
        }
    }
}

/// Fills the defaults of `section` under one element of a nested list or map (the
/// `Defaults` layer cannot know which elements will exist). Values already present win;
/// maps are filled recursively.
fn fill_defaults(element: &mut ConfigValue, section: &'static ConfigMeta) {
    let defaults = defaults_tree(section);
    fill_missing(element, &defaults);
}

fn fill_missing(target: &mut ConfigValue, defaults: &ConfigValue) {
    let (Some(map), Some(defaults)) = (target.as_map_mut(), defaults.as_map()) else {
        return;
    };
    for (key, default) in defaults.iter_shared() {
        match map.get_mut(key) {
            Some(existing) => fill_missing(existing, default),
            None => {
                map.insert(key.clone(), default.clone());
            }
        }
    }
}

/// Whether `key` names a non-flattened field of `meta` directly.
fn answers_directly(meta: &ConfigMeta, key: &str) -> bool {
    meta.fields
        .iter()
        .any(|field| !field.flatten && field.answers_to(key))
}

/// Converts one node in place. The message never contains the input.
fn parse_node(value: &mut ConfigValue, kind: ParseKind) -> Result<(), alloc::string::String> {
    let replacement = match (kind, value.kind()) {
        (ParseKind::Duration, Kind::String(text)) => {
            let duration =
                parse_duration(text).map_err(|error| format!("invalid duration: {error}"))?;
            Kind::Map(
                [
                    (
                        "secs",
                        ConfigValue::from(Number::from_u64(duration.as_secs())),
                    ),
                    (
                        "nanos",
                        ConfigValue::from(Number::from_u64(u64::from(duration.subsec_nanos()))),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        }
        (ParseKind::Duration, Kind::Number(_)) => {
            return Err(
                "a duration needs a unit, for example \"30s\" or \"250ms\"; found a number".into(),
            );
        }
        (ParseKind::Bytes, Kind::String(text)) => {
            let bytes = parse_bytes(text).map_err(|error| format!("invalid byte size: {error}"))?;
            Kind::Number(Number::from_u64(bytes))
        }
        (ParseKind::Bytes, Kind::Number(number)) => {
            if number.as_u64().is_some() {
                return Ok(());
            }
            return Err("a byte size must be a non-negative whole number".into());
        }
        // Maps (an already-converted duration), nulls, and other shapes are left to serde.
        _ => return Ok(()),
    };
    let secret_children = value.meta().secret;
    *value.kind_mut() = replacement;
    if secret_children {
        mark_secret(value);
    }
    let origin = value.meta().origin.clone();
    if let Kind::Map(map) = value.kind_mut() {
        for child in map.values_mut() {
            child.meta_mut().origin = origin.clone();
        }
    }
    Ok(())
}

fn mark_secret(value: &mut ConfigValue) {
    value.meta_mut().secret = true;
    match value.kind_mut() {
        Kind::Array(elements) => elements.iter_mut().for_each(mark_secret),
        Kind::Map(map) => map.values_mut().for_each(mark_secret),
        _ => {}
    }
}

/// Splits the unknown keys of a deserialization into those inside a `deny_unknown`
/// section (first) and the rest (second). Paths are in serde form (after [`normalize`]).
#[must_use]
pub fn split_denied(
    meta: &'static ConfigMeta,
    unknown: Vec<UnknownKey>,
) -> (Vec<UnknownKey>, Vec<UnknownKey>) {
    unknown.into_iter().partition(|key| {
        let parent = &key.path.segments()[..key.path.len().saturating_sub(1)];
        section_at(meta, parent).is_some_and(|section| section.deny_unknown)
    })
}

/// The section that holds the node at `segments`, following nested fields.
fn section_at(meta: &'static ConfigMeta, segments: &[PathSegment]) -> Option<&'static ConfigMeta> {
    let mut current = meta;
    let mut rest = segments;
    while let Some((first, tail)) = rest.split_first() {
        let field: &FieldMeta = current.field(first.as_key()?)?;
        let nested = field.nested?;
        rest = match nested.shape {
            NestedShape::Single => tail,
            // Skip the index or the map key.
            _ => tail.get(1..)?,
        };
        current = nested.section();
    }
    Some(current)
}

/// Merges `overlay` onto `base` (both in document form) honouring each field's
/// `#[config(merge = "...")]` policy, and `options` everywhere else:
///
/// * `replace`: the overlay's value replaces the base's whole (maps included);
/// * `append`: two arrays concatenate, base first;
/// * `deep`: the usual deep merge.
///
/// # Errors
///
/// As [`merge`](crate::merge::merge).
pub fn merge_with_schema(
    mut base: ConfigValue,
    mut overlay: ConfigValue,
    meta: &'static ConfigMeta,
    options: &crate::merge::MergeOptions,
) -> Result<ConfigValue, ConfigError> {
    apply_field_policies(&mut base, &mut overlay, meta, options, 0);
    crate::merge::merge(base, overlay, options)
}

fn apply_field_policies(
    base: &mut ConfigValue,
    overlay: &mut ConfigValue,
    meta: &'static ConfigMeta,
    options: &crate::merge::MergeOptions,
    depth: usize,
) {
    if depth >= MAX_SECTION_DEPTH || !base.is_map() || !overlay.is_map() {
        return;
    }
    let keys: Vec<Arc<str>> = overlay
        .as_map()
        .map(|map| map.iter_shared().map(|(key, _)| key.clone()).collect())
        .unwrap_or_default();
    for key in keys {
        // Layers are canonical (serde keys, flattened sections under their field), but a
        // layer that skipped canonicalization may still use document keys.
        let field = match meta.fields.iter().find(|field| *field.key == *key) {
            Some(field) => field,
            None => match meta.resolve_doc_key(&key) {
                Some(chain) => chain.target(),
                None => continue,
            },
        };
        let base_map = base.as_map_mut().expect("checked above");
        let overlay_map = overlay.as_map_mut().expect("checked above");
        let (Some(lower), Some(upper)) = (base_map.get_mut(&key), overlay_map.get_mut(&key)) else {
            continue;
        };
        match field.merge {
            Some(cgp_config_core::FieldMerge::Replace) => {
                upper
                    .meta_mut()
                    .shadow(lower.meta(), options.max_provenance);
                base_map.remove(&key);
            }
            Some(cgp_config_core::FieldMerge::Append) => {
                if let (Some(first), Some(second)) = (lower.as_array(), upper.as_array()) {
                    let joined: Vec<ConfigValue> = first.iter().chain(second).cloned().collect();
                    let mut meta = upper.meta().clone();
                    meta.shadow(lower.meta(), options.max_provenance);
                    *upper = ConfigValue::new(Kind::Array(joined), meta);
                }
            }
            _ => {
                if let Some(section) = field.single_section() {
                    apply_field_policies(lower, upper, section, options, depth + 1);
                }
            }
        }
    }
}

/// Errors as one: a helper for sources that fold several failures.
#[must_use]
pub fn fold_errors(errors: impl IntoIterator<Item = ConfigError>) -> Option<ConfigError> {
    errors.into_iter().reduce(AggregateError::aggregate)
}

#[cfg(test)]
mod tests;
