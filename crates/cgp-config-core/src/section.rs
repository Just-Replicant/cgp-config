//! Static metadata of a config type: [`ConfigMeta`], [`FieldMeta`], and the
//! [`ConfigSection`] trait that `#[derive(ConfigSection)]` implements.
//!
//! The metadata is `'static` and needs no allocator, so the derive's output works in every
//! tier. Later stages read it:
//!
//! | Stage | Uses |
//! |---|---|
//! | `Defaults` layer | [`FieldMeta::default`] (const defaults only) |
//! | env layer | document keys, [`FieldMeta::env`], [`FieldMeta::env_raw`], flatten |
//! | deserializer | aliases, mounts, flatten, `parse`, `secret`, per-section `deny_unknown` |
//! | interpolation | [`FieldMeta::interpolate`] |
//! | merge (`KeyedMerge`) | [`FieldMeta::merge`] |
//! | validation (`Nested`) | nested sections |
//!
//! A context names the metadata of the type it loads through
//! [`HasRootConfigMeta`](crate::HasRootConfigMeta) (`UseConfigMeta<AppConfig>`), because
//! sources run before the target type is known.
//!
//! ## Keys
//!
//! Every field has three names:
//!
//! * [`name`](FieldMeta::name): the Rust identifier;
//! * [`key`](FieldMeta::key): the key serde expects (after `#[serde(rename)]` /
//!   `rename_all`);
//! * [`doc_key`](FieldMeta::doc_key): the key in documents and env names
//!   (`#[config(mount = "...")]`, otherwise `key`).
//!
//! Before deserialization the tree is normalized: aliases and mounts are renamed to `key`,
//! and the fields of a `#[config(flatten)]` section, which documents write on the parent,
//! are moved under the flattened field.

use core::fmt;

/// A const default value, as written in `#[config(default = ...)]`.
///
/// Only literals and paths to constants are accepted by the derive: a default never runs
/// code and never reads the environment.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ConstDefault {
    /// A boolean.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// An unsigned integer.
    UInt(u64),
    /// A float.
    Float(f64),
    /// A string (also durations and byte sizes in text form: `"30s"`, `"10MiB"`).
    Str(&'static str),
    /// An array of defaults.
    Array(&'static [ConstDefault]),
}

/// A text parser applied to a field before serde sees it (`#[config(parse = "...")]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseKind {
    /// `30s`, `5m`, `1h`, `100ms`, `1s200ms` into a `Duration`. A bare number is rejected.
    Duration,
    /// `10MiB`, `1KB`, `512` into a byte count (`u64`). A bare number is bytes.
    Bytes,
}

impl ParseKind {
    /// `duration` or `bytes`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Duration => "duration",
            Self::Bytes => "bytes",
        }
    }
}

/// When a changed field takes effect (`#[config(reload = "...")]`, used by reload in 0.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReloadPolicy {
    /// The new value is published by a reload. The default.
    #[default]
    Live,
    /// A change needs a restart; a reload that changes it is rejected.
    Restart,
}

/// How a field merges across layers (`#[config(merge = "...")]`, read by `KeyedMerge`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FieldMerge {
    /// Maps merge recursively; scalars and arrays replace (the default policy).
    Deep,
    /// The later layer replaces the whole value, maps included.
    Replace,
    /// Arrays concatenate (base, then overlay); maps merge recursively.
    Append,
}

/// How a nested section is held by its parent field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NestedShape {
    /// One section (`Http`, `Option<Http>`, `Box<Http>`).
    Single,
    /// A sequence of sections (`Vec<Http>`).
    Seq,
    /// A map of sections (`BTreeMap<String, Http>`).
    Map,
}

/// A nested section: its shape and its metadata.
///
/// The metadata is reached through a function pointer so recursive types do not make the
/// constants cyclic.
#[derive(Clone, Copy)]
pub struct NestedMeta {
    /// How the parent holds the section.
    pub shape: NestedShape,
    /// The section's metadata.
    pub meta: fn() -> &'static ConfigMeta,
}

impl NestedMeta {
    /// A nested section.
    #[must_use]
    pub const fn new(shape: NestedShape, meta: fn() -> &'static ConfigMeta) -> Self {
        Self { shape, meta }
    }

    /// The section's metadata.
    #[must_use]
    pub fn section(&self) -> &'static ConfigMeta {
        (self.meta)()
    }
}

impl fmt::Debug for NestedMeta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NestedMeta")
            .field("shape", &self.shape)
            .field("section", &self.section().name)
            .finish()
    }
}

impl PartialEq for NestedMeta {
    fn eq(&self, other: &Self) -> bool {
        self.shape == other.shape && core::ptr::eq(self.section(), other.section())
    }
}

/// Metadata of one field.
///
/// Built with `const` builders so the derive's output stays valid when fields are added:
///
/// ```
/// use cgp_config_core::{ConstDefault, FieldMeta};
///
/// const PORT: FieldMeta = FieldMeta::new("port", "port")
///     .with_doc("Port to bind.")
///     .with_default(ConstDefault::UInt(8080))
///     .with_env("PORT");
/// assert_eq!(PORT.doc_key, "port");
/// assert_eq!(PORT.env, Some("PORT"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FieldMeta {
    /// The Rust identifier.
    pub name: &'static str,
    /// The key serde expects.
    pub key: &'static str,
    /// The key in documents and env names (the mount, or `key`).
    pub doc_key: &'static str,
    /// The field's doc comment.
    pub doc: &'static str,
    /// The field's type as written, for documentation.
    pub ty: &'static str,
    /// The const default.
    pub default: Option<ConstDefault>,
    /// Env name suffix replacing the field segment (`env = "PORT"`).
    pub env: Option<&'static str>,
    /// Exact, process-global env name (`env_raw = "PORT"`).
    pub env_raw: Option<&'static str>,
    /// Old names accepted from any layer, with a deprecation warning.
    pub aliases: &'static [&'static str],
    /// Deprecation note shown when an alias is used.
    pub deprecated: Option<&'static str>,
    /// The value is a secret (or a secret reference): never echoed.
    pub secret: bool,
    /// When a change takes effect.
    pub reload: ReloadPolicy,
    /// Merge policy across layers, when not the context's default.
    pub merge: Option<FieldMerge>,
    /// `${...}` in this field's strings is expanded (with `InterpolateOptedIn`).
    pub interpolate: bool,
    /// Text parser applied before serde.
    pub parse: Option<ParseKind>,
    /// The section's fields appear on the parent (in documents and env names).
    pub flatten: bool,
    /// The field is an `Option`.
    pub optional: bool,
    /// The field is a nested section.
    pub nested: Option<NestedMeta>,
    /// The field has no default of any kind (not an `Option`, no `#[config(default)]`,
    /// no `#[serde(default)]`), so a load without it fails. Lets the deserializer report
    /// every missing field at once instead of serde's first one.
    pub required: bool,
}

impl FieldMeta {
    /// A field named `name` that serde reads as `key`. Every other property is unset.
    #[must_use]
    pub const fn new(name: &'static str, key: &'static str) -> Self {
        Self {
            name,
            key,
            doc_key: key,
            doc: "",
            ty: "",
            default: None,
            env: None,
            env_raw: None,
            aliases: &[],
            deprecated: None,
            secret: false,
            reload: ReloadPolicy::Live,
            merge: None,
            interpolate: false,
            parse: None,
            flatten: false,
            optional: false,
            nested: None,
            required: false,
        }
    }

    /// Sets the document key (`mount`).
    #[must_use]
    pub const fn with_doc_key(mut self, doc_key: &'static str) -> Self {
        self.doc_key = doc_key;
        self
    }

    /// Sets the doc comment.
    #[must_use]
    pub const fn with_doc(mut self, doc: &'static str) -> Self {
        self.doc = doc;
        self
    }

    /// Sets the type name.
    #[must_use]
    pub const fn with_type(mut self, ty: &'static str) -> Self {
        self.ty = ty;
        self
    }

    /// Sets the const default.
    #[must_use]
    pub const fn with_default(mut self, default: ConstDefault) -> Self {
        self.default = Some(default);
        self
    }

    /// Sets the env suffix.
    #[must_use]
    pub const fn with_env(mut self, env: &'static str) -> Self {
        self.env = Some(env);
        self
    }

    /// Sets the raw env name.
    #[must_use]
    pub const fn with_env_raw(mut self, env_raw: &'static str) -> Self {
        self.env_raw = Some(env_raw);
        self
    }

    /// Sets the aliases.
    #[must_use]
    pub const fn with_aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    /// Sets the deprecation note.
    #[must_use]
    pub const fn with_deprecated(mut self, note: &'static str) -> Self {
        self.deprecated = Some(note);
        self
    }

    /// Marks the field secret.
    #[must_use]
    pub const fn with_secret(mut self, secret: bool) -> Self {
        self.secret = secret;
        self
    }

    /// Sets the reload policy.
    #[must_use]
    pub const fn with_reload(mut self, reload: ReloadPolicy) -> Self {
        self.reload = reload;
        self
    }

    /// Sets the merge policy.
    #[must_use]
    pub const fn with_merge(mut self, merge: FieldMerge) -> Self {
        self.merge = Some(merge);
        self
    }

    /// Opts the field into interpolation.
    #[must_use]
    pub const fn with_interpolate(mut self, interpolate: bool) -> Self {
        self.interpolate = interpolate;
        self
    }

    /// Sets the text parser.
    #[must_use]
    pub const fn with_parse(mut self, parse: ParseKind) -> Self {
        self.parse = Some(parse);
        self
    }

    /// Marks the field flattened.
    #[must_use]
    pub const fn with_flatten(mut self, flatten: bool) -> Self {
        self.flatten = flatten;
        self
    }

    /// Marks the field optional.
    #[must_use]
    pub const fn with_optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    /// Marks the field a nested section.
    #[must_use]
    pub const fn with_nested(mut self, nested: NestedMeta) -> Self {
        self.nested = Some(nested);
        self
    }

    /// Marks the field as required (see [`required`](Self::required)).
    #[must_use]
    pub const fn with_required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// The nested section held as a single value, if any.
    #[must_use]
    pub fn single_section(&self) -> Option<&'static ConfigMeta> {
        self.nested
            .filter(|nested| nested.shape == NestedShape::Single)
            .map(|nested| nested.section())
    }

    /// Whether `doc_key` names this field in a document: its document key or an alias.
    #[must_use]
    pub fn answers_to(&self, doc_key: &str) -> bool {
        self.doc_key == doc_key || self.aliases.contains(&doc_key)
    }
}

/// Metadata of a config type (a section).
///
/// ```
/// use cgp_config_core::{ConfigMeta, ConfigSection, ConstDefault, FieldMeta};
///
/// struct Http;
///
/// impl ConfigSection for Http {
///     const META: &'static ConfigMeta = &ConfigMeta::new(
///         "Http",
///         &[FieldMeta::new("port", "port").with_default(ConstDefault::UInt(8080))],
///     )
///     .with_section("http");
/// }
///
/// assert_eq!(Http::META.section, Some("http"));
/// assert_eq!(Http::META.field("port").unwrap().default, Some(ConstDefault::UInt(8080)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ConfigMeta {
    /// The Rust type name.
    pub name: &'static str,
    /// The section's default key (`#[config(section = "http")]`).
    pub section: Option<&'static str>,
    /// The type's doc comment.
    pub doc: &'static str,
    /// The schema version (`#[config(version = N)]`, default 1).
    pub version: u32,
    /// The default env prefix (`#[config(env_prefix = "APP")]`).
    pub env_prefix: Option<&'static str>,
    /// Unknown keys inside this section fail the load, whatever the global policy.
    pub deny_unknown: bool,
    /// The fields, in declaration order. `#[config(skip)]` fields are absent.
    pub fields: &'static [FieldMeta],
}

impl ConfigMeta {
    /// A type named `name` with `fields`.
    #[must_use]
    pub const fn new(name: &'static str, fields: &'static [FieldMeta]) -> Self {
        Self {
            name,
            section: None,
            doc: "",
            version: 1,
            env_prefix: None,
            deny_unknown: false,
            fields,
        }
    }

    /// Sets the section key.
    #[must_use]
    pub const fn with_section(mut self, section: &'static str) -> Self {
        self.section = Some(section);
        self
    }

    /// Sets the doc comment.
    #[must_use]
    pub const fn with_doc(mut self, doc: &'static str) -> Self {
        self.doc = doc;
        self
    }

    /// Sets the schema version.
    #[must_use]
    pub const fn with_version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    /// Sets the env prefix.
    #[must_use]
    pub const fn with_env_prefix(mut self, prefix: &'static str) -> Self {
        self.env_prefix = Some(prefix);
        self
    }

    /// Sets `deny_unknown`.
    #[must_use]
    pub const fn with_deny_unknown(mut self, deny: bool) -> Self {
        self.deny_unknown = deny;
        self
    }

    /// The field serde reads as `key`.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&'static FieldMeta> {
        self.fields.iter().find(|field| field.key == key)
    }

    /// Resolves a document key on this section, looking through flattened sections.
    ///
    /// Returns the chain of fields from this section down to the field the key names
    /// (one element unless flattened sections are involved), or `None` for an undeclared
    /// key. Aliases resolve to their field.
    ///
    /// ```
    /// use cgp_config_core::{ConfigMeta, ConfigSection, FieldMeta, NestedMeta, NestedShape};
    ///
    /// struct Http;
    /// impl ConfigSection for Http {
    ///     const META: &'static ConfigMeta =
    ///         &ConfigMeta::new("Http", &[FieldMeta::new("port", "port")]);
    /// }
    /// struct App;
    /// impl ConfigSection for App {
    ///     const META: &'static ConfigMeta = &ConfigMeta::new(
    ///         "App",
    ///         &[FieldMeta::new("http", "http")
    ///             .with_flatten(true)
    ///             .with_nested(NestedMeta::new(NestedShape::Single, <Http as ConfigSection>::meta))],
    ///     );
    /// }
    ///
    /// let chain = App::META.resolve_doc_key("port").unwrap();
    /// let names: Vec<_> = chain.iter().map(|field| field.name).collect();
    /// assert_eq!(names, ["http", "port"]);
    /// ```
    #[must_use]
    pub fn resolve_doc_key(&self, doc_key: &str) -> Option<FieldChain> {
        let mut chain = FieldChain::new();
        self.resolve_into(doc_key, &mut chain, 0).then_some(chain)
    }

    fn resolve_into(&self, doc_key: &str, chain: &mut FieldChain, depth: usize) -> bool {
        if depth >= FieldChain::CAPACITY {
            return false;
        }
        for field in self.fields {
            if !field.flatten && field.answers_to(doc_key) {
                chain.push(field);
                return true;
            }
        }
        for field in self.fields.iter().filter(|field| field.flatten) {
            let Some(section) = field.single_section() else {
                continue;
            };
            chain.push(field);
            if section.resolve_into(doc_key, chain, depth + 1) {
                return true;
            }
            chain.pop();
        }
        false
    }

    /// Whether any field, here or in a nested section, carries `predicate`.
    ///
    /// Recursion stops at [`FieldChain::CAPACITY`] levels, so a recursive type terminates.
    #[must_use]
    pub fn any_field(&self, predicate: &dyn Fn(&FieldMeta) -> bool) -> bool {
        fn walk(meta: &ConfigMeta, predicate: &dyn Fn(&FieldMeta) -> bool, depth: usize) -> bool {
            depth < FieldChain::CAPACITY
                && meta.fields.iter().any(|field| {
                    predicate(field)
                        || field
                            .nested
                            .is_some_and(|nested| walk(nested.section(), predicate, depth + 1))
                })
        }
        walk(self, predicate, 0)
    }
}

/// A short chain of fields from a section down through flattened sections, as returned by
/// [`ConfigMeta::resolve_doc_key`]. No allocation; at most [`CAPACITY`](Self::CAPACITY)
/// levels.
#[derive(Clone, Copy)]
pub struct FieldChain {
    fields: [Option<&'static FieldMeta>; Self::CAPACITY],
    len: usize,
}

impl FieldChain {
    /// The deepest flatten nesting followed.
    pub const CAPACITY: usize = 16;

    const fn new() -> Self {
        Self {
            fields: [None; Self::CAPACITY],
            len: 0,
        }
    }

    fn push(&mut self, field: &'static FieldMeta) {
        if self.len < Self::CAPACITY {
            self.fields[self.len] = Some(field);
            self.len += 1;
        }
    }

    fn pop(&mut self) {
        if self.len > 0 {
            self.len -= 1;
            self.fields[self.len] = None;
        }
    }

    /// The fields, outermost first.
    pub fn iter(&self) -> impl Iterator<Item = &'static FieldMeta> + '_ {
        self.fields[..self.len].iter().filter_map(|field| *field)
    }

    /// The field the key names (the last of the chain).
    #[must_use]
    pub fn target(&self) -> &'static FieldMeta {
        self.fields[self.len - 1].expect("a resolved chain is never empty")
    }

    /// Number of fields.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the chain is empty (never, for a resolved key).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether the key was found through a flattened section.
    #[must_use]
    pub fn is_flattened(&self) -> bool {
        self.len > 1
    }
}

impl fmt::Debug for FieldChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.iter().map(|field| field.name))
            .finish()
    }
}

/// A config type with static metadata. Implemented by `#[derive(ConfigSection)]`.
///
/// The trait is usable without the derive; implement [`META`](Self::META) by hand with the
/// const builders of [`ConfigMeta`] and [`FieldMeta`].
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a config section",
    label = "no `ConfigSection` metadata",
    note = "derive it with `#[derive(ConfigSection)]`, or drop `#[config(nested)]` / `flatten` to read the field as a plain value"
)]
pub trait ConfigSection {
    /// The type's metadata.
    const META: &'static ConfigMeta;

    /// [`META`](Self::META) as a function, for [`NestedMeta`].
    #[must_use]
    fn meta() -> &'static ConfigMeta {
        Self::META
    }
}

/// The names of every missing field of a partial config, collected by
/// [`FinalizeOptionalAll`].
///
/// No allocation and a small error value: the type holds the field list of the partial
/// struct and a bit per field (the first [`CAPACITY`](Self::CAPACITY) fields), so every
/// missing field is reported, not only the first.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MissingFields {
    fields: &'static [&'static str],
    mask: u128,
    overflow: u32,
}

impl MissingFields {
    /// Fields tracked by name; missing fields past this index (or not in the list) are
    /// only counted.
    pub const CAPACITY: usize = 128;

    /// No missing field, for a struct with these fields (in declaration order).
    #[must_use]
    pub const fn new(fields: &'static [&'static str]) -> Self {
        Self {
            fields,
            mask: 0,
            overflow: 0,
        }
    }

    /// Records the field at `index` of the field list as missing.
    pub fn mark(&mut self, index: usize) {
        if index < Self::CAPACITY && index < self.fields.len() {
            self.mask |= 1 << index;
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    /// Records a missing field by name.
    pub fn push(&mut self, name: &'static str) {
        match self.fields.iter().position(|field| *field == name) {
            Some(index) => self.mark(index),
            None => self.overflow = self.overflow.saturating_add(1),
        }
    }

    /// The missing fields that are tracked by name, in declaration order.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        let mask = self.mask;
        let fields = self.fields;
        fields
            .iter()
            .take(Self::CAPACITY)
            .enumerate()
            .filter(move |(index, _)| mask & (1 << index) != 0)
            .map(|(_, name)| *name)
    }

    /// Number of missing fields, including those only counted.
    #[must_use]
    pub fn count(&self) -> usize {
        self.mask.count_ones() as usize + self.overflow as usize
    }

    /// Whether nothing is missing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// `Ok(())` when nothing is missing.
    ///
    /// # Errors
    ///
    /// `self` when at least one field is missing.
    pub fn into_result(self) -> Result<(), Self> {
        if self.is_empty() { Ok(()) } else { Err(self) }
    }
}

impl Default for MissingFields {
    fn default() -> Self {
        Self::new(&[])
    }
}

impl fmt::Debug for MissingFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.names()).finish()?;
        if self.overflow > 0 {
            write!(f, " (+{} more)", self.overflow)?;
        }
        Ok(())
    }
}

impl fmt::Display for MissingFields {
    /// `missing fields: a, b (+3 more)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let noun = if self.count() == 1 { "field" } else { "fields" };
        write!(f, "missing {noun}: ")?;
        for (index, name) in self.names().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            f.write_str(name)?;
        }
        if self.overflow > 0 {
            write!(f, " (+{} more)", self.overflow)?;
        }
        Ok(())
    }
}

impl core::error::Error for MissingFields {}

/// Turns a partial config (every field an `Option`) into the full type, reporting
/// **every** missing field rather than the first one.
///
/// `#[config(partial)]` generates `{Name}Partial` and this impl. It is the typed path for
/// `no_std` targets without an allocator, where the dynamic value tree is not available.
pub trait FinalizeOptionalAll {
    /// The complete type.
    type Output;

    /// The complete value.
    ///
    /// # Errors
    ///
    /// Every field that is still `None`.
    fn finalize_optional_all(self) -> Result<Self::Output, MissingFields>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Inner;
    impl ConfigSection for Inner {
        const META: &'static ConfigMeta = &ConfigMeta::new(
            "Inner",
            &[
                FieldMeta::new("port", "port").with_aliases(&["listen_port"]),
                FieldMeta::new("token", "token").with_secret(true),
            ],
        );
    }

    struct Outer;
    impl ConfigSection for Outer {
        const META: &'static ConfigMeta = &ConfigMeta::new(
            "Outer",
            &[
                FieldMeta::new("name", "name"),
                FieldMeta::new("inner", "inner")
                    .with_flatten(true)
                    .with_nested(NestedMeta::new(
                        NestedShape::Single,
                        <Inner as ConfigSection>::meta,
                    )),
                FieldMeta::new("db", "database").with_doc_key("db"),
            ],
        )
        .with_version(2);
    }

    /// A type that contains itself, to prove recursion terminates.
    struct Tree;
    impl ConfigSection for Tree {
        const META: &'static ConfigMeta = &ConfigMeta::new(
            "Tree",
            &[FieldMeta::new("child", "child")
                .with_flatten(true)
                .with_nested(NestedMeta::new(
                    NestedShape::Single,
                    <Tree as ConfigSection>::meta,
                ))],
        );
    }

    #[test]
    fn resolves_plain_aliased_flattened_and_mounted_keys() {
        let meta = Outer::META;
        assert_eq!(meta.version, 2);
        assert_eq!(meta.resolve_doc_key("name").unwrap().target().name, "name");
        let chain = meta.resolve_doc_key("listen_port").unwrap();
        assert!(chain.is_flattened());
        assert_eq!(chain.len(), 2);
        assert_eq!(chain.target().name, "port");
        assert_eq!(format_chain(&chain), "[\"inner\", \"port\"]");
        assert_eq!(meta.resolve_doc_key("db").unwrap().target().key, "database");
        assert!(meta.resolve_doc_key("database").is_none());
        assert!(meta.resolve_doc_key("inner").is_none());
        assert!(meta.field("database").is_some());
        assert!(meta.any_field(&|field| field.secret));
        assert!(!meta.any_field(&|field| field.interpolate));
    }

    #[test]
    fn recursive_metadata_terminates() {
        assert!(Tree::META.resolve_doc_key("missing").is_none());
        assert!(!Tree::META.any_field(&|field| field.secret));
    }

    fn format_chain(chain: &FieldChain) -> alloc_free::Buf {
        use core::fmt::Write;
        let mut buf = alloc_free::Buf::default();
        write!(buf, "{chain:?}").unwrap();
        buf
    }

    /// A tiny fixed buffer so this test runs without `alloc`.
    mod alloc_free {
        pub(super) struct Buf {
            bytes: [u8; 64],
            len: usize,
        }

        impl Default for Buf {
            fn default() -> Self {
                Self {
                    bytes: [0; 64],
                    len: 0,
                }
            }
        }

        impl core::fmt::Write for Buf {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                let end = self.len + s.len();
                self.bytes
                    .get_mut(self.len..end)
                    .ok_or(core::fmt::Error)?
                    .copy_from_slice(s.as_bytes());
                self.len = end;
                Ok(())
            }
        }

        impl PartialEq<&str> for Buf {
            fn eq(&self, other: &&str) -> bool {
                &self.bytes[..self.len] == other.as_bytes()
            }
        }

        impl core::fmt::Debug for Buf {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("?"))
            }
        }
    }

    #[test]
    fn missing_fields_cap_and_display() {
        let mut missing = MissingFields::new(&["host", "port", "name"]);
        assert!(missing.into_result().is_ok());
        missing.mark(2);
        missing.push("host");
        missing.push("unlisted");
        missing.mark(99);
        assert_eq!(missing.count(), 4);
        assert!(missing.names().eq(["host", "name"]));
        assert!(missing.into_result().is_err());
        let mut buf = alloc_free::Buf::default();
        core::fmt::Write::write_fmt(&mut buf, format_args!("{missing}")).unwrap();
        assert_eq!(buf, "missing fields: host, name (+2 more)");
        let mut one = MissingFields::new(&["port"]);
        one.push("port");
        let mut buf = alloc_free::Buf::default();
        core::fmt::Write::write_fmt(&mut buf, format_args!("{one}")).unwrap();
        assert_eq!(buf, "missing field: port");
    }

    #[test]
    fn parse_kind_names_and_builders() {
        assert_eq!(ParseKind::Duration.name(), "duration");
        assert_eq!(ParseKind::Bytes.name(), "bytes");
        let field = FieldMeta::new("a", "a")
            .with_type("u16")
            .with_doc("doc")
            .with_env_raw("A")
            .with_deprecated("old")
            .with_reload(ReloadPolicy::Restart)
            .with_merge(FieldMerge::Append)
            .with_interpolate(true)
            .with_parse(ParseKind::Bytes)
            .with_optional(true);
        assert_eq!(field.reload, ReloadPolicy::Restart);
        assert_eq!(field.merge, Some(FieldMerge::Append));
        assert!(field.single_section().is_none());
        let meta = ConfigMeta::new("M", &[])
            .with_doc("d")
            .with_env_prefix("APP")
            .with_deny_unknown(true);
        assert!(meta.deny_unknown);
        assert_eq!(meta.env_prefix, Some("APP"));
        let nested = NestedMeta::new(NestedShape::Seq, <Inner as ConfigSection>::meta);
        assert_eq!(nested, nested);
    }
}
