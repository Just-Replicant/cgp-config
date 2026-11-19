//! `${...}` interpolation of opted-in fields: the lexer ([`lex`]), the expander
//! ([`interpolate_tree`]), and the [`InterpolateOptedIn`] provider (feature
//! `interpolate`).
//!
//! | Form | Meaning |
//! |---|---|
//! | `$$` | a literal `$` |
//! | `${VAR}` | the env snapshot's `VAR`; unset is `ConfigError::Interpolate` |
//! | `${VAR:-text}` | `VAR`, or `text` (itself expanded) when unset |
//! | `${VAR:?message}` | `VAR`, or `ConfigError::Interpolate` with `message` |
//! | `${ref:http.host}` | the value at another document path, after that path is expanded |
//!
//! Only strings under fields marked `#[config(interpolate)]` in the context's root schema
//! are scanned; every other string, `${` included, stays literal. A `$` not followed by
//! `$` or `{` is literal. Cycles of references are `ConfigError::InterpolateCycle`;
//! nesting deeper than `LoadLimits::max_interpolation_depth` is
//! `ConfigError::LimitExceeded`. Expanded nodes get `Meta::expanded` and are never scanned
//! again. A reference to a secret node is refused, so a secret cannot leak into a
//! non-secret string.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use cgp_config_core::{
    ConfigError, ConfigMeta, EnvSnapshot, FieldChain, HasConfigValueType, HasEnvSnapshot,
    HasLoadLimits, HasRootConfigMeta, Interpolator, InterpolatorComponent, KeyPath, LoadLimits,
    NestedShape, PathSegment, accumulate,
};
use cgp_fork::prelude::*;

use crate::{ConfigValue, Kind};

/// One piece of an interpolated string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece<'a> {
    /// Literal text.
    Text(&'a str),
    /// `$$`.
    Dollar,
    /// `${VAR}`, `${VAR:-default}`, or `${VAR:?message}`.
    Env {
        /// The variable name.
        name: &'a str,
        /// What happens when it is unset.
        fallback: Fallback<'a>,
    },
    /// `${ref:a.b}`: the document path.
    Ref(&'a str),
}

/// The unset-variable behavior of [`Piece::Env`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback<'a> {
    /// Unset is an error.
    Required,
    /// Unset expands this template.
    Default(&'a str),
    /// Unset is an error with this message.
    Error(&'a str),
}

/// A malformed template. Positions are byte offsets; the text itself is not kept, so the
/// error is safe on any path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LexError {
    /// `${` without its `}`.
    Unterminated {
        /// Offset of the `$`.
        at: usize,
    },
    /// `${}` or `${ref:}`.
    Empty {
        /// Offset of the `$`.
        at: usize,
    },
    /// A variable name that is not `[A-Za-z_][A-Za-z0-9_]*`.
    InvalidName {
        /// Offset of the `$`.
        at: usize,
    },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unterminated { at } => write!(f, "unterminated `${{` at byte {at}"),
            Self::Empty { at } => write!(f, "empty `${{}}` at byte {at}"),
            Self::InvalidName { at } => write!(f, "invalid variable name at byte {at}"),
        }
    }
}

impl core::error::Error for LexError {}

/// Splits `text` into pieces.
///
/// ```
/// use cgp_config_value::interpolate::{Fallback, Piece, lex};
///
/// let pieces = lex("http://${HOST:-localhost}:$${ref:http.port}").unwrap();
/// assert_eq!(pieces, [
///     Piece::Text("http://"),
///     Piece::Env { name: "HOST", fallback: Fallback::Default("localhost") },
///     Piece::Text(":"),
///     Piece::Dollar,
///     Piece::Text("{ref:http.port}"),
/// ]);
/// ```
///
/// # Errors
///
/// A [`LexError`] for a malformed `${...}`.
pub fn lex(text: &str) -> Result<Vec<Piece<'_>>, LexError> {
    let bytes = text.as_bytes();
    let mut pieces = Vec::new();
    let mut literal_start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'$' {
            index += 1;
            continue;
        }
        match bytes.get(index + 1) {
            Some(b'$') => {
                push_text(&mut pieces, &text[literal_start..index]);
                pieces.push(Piece::Dollar);
                index += 2;
                literal_start = index;
            }
            Some(b'{') => {
                push_text(&mut pieces, &text[literal_start..index]);
                let end =
                    matching_brace(bytes, index + 2).ok_or(LexError::Unterminated { at: index })?;
                pieces.push(parse_expression(&text[index + 2..end], index)?);
                index = end + 1;
                literal_start = index;
            }
            _ => index += 1,
        }
    }
    push_text(&mut pieces, &text[literal_start..]);
    Ok(pieces)
}

fn push_text<'a>(pieces: &mut Vec<Piece<'a>>, text: &'a str) {
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
}

/// The index of the `}` closing a `${` whose body starts at `start`, counting nested
/// `${` (a default may contain expressions) and skipping `$$`.
fn matching_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b'$' if bytes.get(index + 1) == Some(&b'$') => index += 1,
            b'$' if bytes.get(index + 1) == Some(&b'{') => {
                depth += 1;
                index += 1;
            }
            b'}' if depth == 0 => return Some(index),
            b'}' => depth -= 1,
            _ => {}
        }
        index += 1;
    }
    None
}

fn parse_expression(body: &str, at: usize) -> Result<Piece<'_>, LexError> {
    if let Some(path) = body.strip_prefix("ref:") {
        let path = path.trim();
        return if path.is_empty() {
            Err(LexError::Empty { at })
        } else {
            Ok(Piece::Ref(path))
        };
    }
    let (name, fallback) = match (body.find(":-"), body.find(":?")) {
        (Some(d), e) if e.is_none_or(|e| d < e) => (&body[..d], Fallback::Default(&body[d + 2..])),
        (_, Some(e)) => (&body[..e], Fallback::Error(&body[e + 2..])),
        _ => (body, Fallback::Required),
    };
    if name.is_empty() {
        return Err(LexError::Empty { at });
    }
    let mut chars = name.chars();
    let valid_start = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    if !valid_start || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(LexError::InvalidName { at });
    }
    Ok(Piece::Env { name, fallback })
}

/// Expands every opted-in string of `tree` (document form) against `meta`, reading
/// variables from `env`. See the [module docs](self).
///
/// # Errors
///
/// Every interpolation failure of the tree, together.
pub fn interpolate_tree(
    tree: ConfigValue,
    meta: &'static ConfigMeta,
    env: &EnvSnapshot,
    limits: &LoadLimits,
) -> Result<ConfigValue, ConfigError> {
    let mut targets = Vec::new();
    collect_targets(
        &tree,
        meta,
        &mut KeyPath::root(),
        false,
        &mut targets,
        0,
        limits,
    );
    if targets.is_empty() {
        return Ok(tree);
    }
    let mut expander = Expander {
        tree: &tree,
        env,
        limits,
        opted_in: targets.iter().cloned().collect(),
        state: BTreeMap::new(),
    };
    let mut errors = None;
    let mut results = Vec::with_capacity(targets.len());
    for path in &targets {
        match expander.expand_path(path, 0) {
            Ok(text) => results.push((path.clone(), text)),
            Err(error) => errors = accumulate(errors, error),
        }
    }
    if let Some(error) = errors {
        return Err(error);
    }
    let mut tree = tree;
    for (path, text) in results {
        if let Some(node) = node_mut(&mut tree, &path) {
            let mut meta = node.meta().clone();
            meta.expanded = true;
            *node = ConfigValue::new(Kind::String(text.into()), meta);
        }
    }
    Ok(tree)
}

/// Paths of the strings to expand: strings under an opted-in field that are not yet
/// expanded.
fn collect_targets(
    node: &ConfigValue,
    meta: &'static ConfigMeta,
    path: &mut KeyPath,
    opted_in: bool,
    out: &mut Vec<KeyPath>,
    depth: usize,
    limits: &LoadLimits,
) {
    if depth > limits.max_depth {
        return;
    }
    match node.kind() {
        Kind::String(_) if opted_in && !node.meta().expanded => out.push(path.clone()),
        Kind::Array(elements) if opted_in => {
            for (index, element) in elements.iter().enumerate() {
                path.push(PathSegment::Index(index));
                collect_targets(element, meta, path, true, out, depth + 1, limits);
                path.pop();
            }
        }
        Kind::Map(map) if opted_in => {
            for (key, value) in map.iter() {
                path.push(key);
                collect_targets(value, meta, path, true, out, depth + 1, limits);
                path.pop();
            }
        }
        Kind::Map(map) => {
            for (key, value) in map.iter() {
                let Some(chain): Option<FieldChain> = meta.resolve_doc_key(key) else {
                    continue;
                };
                let field = chain.target();
                path.push(key);
                if field.interpolate {
                    collect_targets(value, meta, path, true, out, depth + 1, limits);
                } else if let Some(nested) = field.nested {
                    let section = nested.section();
                    match (nested.shape, value.kind()) {
                        (NestedShape::Single, _) => {
                            collect_targets(value, section, path, false, out, depth + 1, limits);
                        }
                        (NestedShape::Seq, Kind::Array(elements)) => {
                            for (index, element) in elements.iter().enumerate() {
                                path.push(PathSegment::Index(index));
                                collect_targets(
                                    element,
                                    section,
                                    path,
                                    false,
                                    out,
                                    depth + 2,
                                    limits,
                                );
                                path.pop();
                            }
                        }
                        (NestedShape::Map, Kind::Map(entries)) => {
                            for (name, entry) in entries.iter() {
                                path.push(name);
                                collect_targets(
                                    entry,
                                    section,
                                    path,
                                    false,
                                    out,
                                    depth + 2,
                                    limits,
                                );
                                path.pop();
                            }
                        }
                        _ => {}
                    }
                }
                path.pop();
            }
        }
        _ => {}
    }
}

fn node_mut<'a>(tree: &'a mut ConfigValue, path: &KeyPath) -> Option<&'a mut ConfigValue> {
    let mut node = tree;
    for segment in path {
        node = match (node.kind_mut(), segment) {
            (Kind::Map(map), PathSegment::Key(key)) => map.get_mut(key)?,
            (Kind::Array(elements), PathSegment::Index(index)) => elements.get_mut(*index)?,
            _ => return None,
        };
    }
    Some(node)
}

enum State {
    InProgress,
    Done(String),
}

struct Expander<'a> {
    tree: &'a ConfigValue,
    env: &'a EnvSnapshot,
    limits: &'a LoadLimits,
    opted_in: alloc::collections::BTreeSet<KeyPath>,
    state: BTreeMap<KeyPath, State>,
}

impl Expander<'_> {
    fn expand_path(&mut self, path: &KeyPath, depth: usize) -> Result<String, ConfigError> {
        match self.state.get(path) {
            Some(State::Done(text)) => return Ok(text.clone()),
            Some(State::InProgress) => {
                return Err(ConfigError::InterpolateCycle { path: path.clone() });
            }
            None => {}
        }
        let node = self
            .tree
            .get_path(path)
            .ok_or_else(|| interpolate_error(path, "path vanished"))?;
        let Some(text) = node.as_str() else {
            return Err(interpolate_error(path, "expected a string"));
        };
        if node.meta().expanded {
            return Ok(text.to_string());
        }
        self.state.insert(path.clone(), State::InProgress);
        let result = self.expand_template(text, path, depth);
        match &result {
            Ok(expanded) => {
                self.state
                    .insert(path.clone(), State::Done(expanded.clone()));
            }
            Err(_) => {
                self.state.remove(path);
            }
        }
        result
    }

    fn expand_template(
        &mut self,
        text: &str,
        path: &KeyPath,
        depth: usize,
    ) -> Result<String, ConfigError> {
        if depth > self.limits.max_interpolation_depth {
            return Err(ConfigError::limit_exceeded(
                "max_interpolation_depth",
                self.limits.max_interpolation_depth,
            ));
        }
        let pieces = lex(text).map_err(|error| interpolate_error(path, &error.to_string()))?;
        let mut out = String::with_capacity(text.len());
        let mut errors = None;
        for piece in pieces {
            match piece {
                Piece::Text(text) => out.push_str(text),
                Piece::Dollar => out.push('$'),
                Piece::Env { name, fallback } => match (self.env.get(name), fallback) {
                    (Some(value), _) => out.push_str(value),
                    (None, Fallback::Default(default)) => {
                        match self.expand_template(default, path, depth + 1) {
                            Ok(text) => out.push_str(&text),
                            Err(error) => errors = accumulate(errors, error),
                        }
                    }
                    (None, Fallback::Error(message)) => {
                        errors = accumulate(errors, interpolate_error(path, message));
                    }
                    (None, Fallback::Required) => {
                        errors = accumulate(
                            errors,
                            interpolate_error(
                                path,
                                &format!("environment variable `{name}` is not set"),
                            ),
                        );
                    }
                },
                Piece::Ref(target) => match self.reference(target, path, depth) {
                    Ok(text) => out.push_str(&text),
                    Err(error) => errors = accumulate(errors, error),
                },
            }
        }
        errors.map_or(Ok(out), Err)
    }

    fn reference(
        &mut self,
        target: &str,
        from: &KeyPath,
        depth: usize,
    ) -> Result<String, ConfigError> {
        let Ok(target_path) = KeyPath::parse(target) else {
            return Err(interpolate_error(from, "invalid reference path"));
        };
        if self.opted_in.contains(&target_path) {
            if depth + 1 > self.limits.max_interpolation_depth {
                return Err(ConfigError::limit_exceeded(
                    "max_interpolation_depth",
                    self.limits.max_interpolation_depth,
                ));
            }
            let node_secret = self
                .tree
                .get_path(&target_path)
                .is_some_and(|n| n.meta().secret);
            if node_secret {
                return Err(interpolate_error(from, "a reference cannot name a secret"));
            }
            return self.expand_path(&target_path, depth + 1);
        }
        let node = self.tree.get_path(&target_path).ok_or_else(|| {
            interpolate_error(
                from,
                &format!("reference to a missing path `{target_path}`"),
            )
        })?;
        if node.meta().secret {
            return Err(interpolate_error(from, "a reference cannot name a secret"));
        }
        match node.kind() {
            Kind::String(text) => Ok(text.to_string()),
            Kind::Bool(value) => Ok(value.to_string()),
            Kind::Number(number) => Ok(number.to_string()),
            _ => Err(interpolate_error(
                from,
                &format!(
                    "reference `{target_path}` is a {}, not a scalar",
                    node.kind_name()
                ),
            )),
        }
    }
}

fn interpolate_error(path: &KeyPath, message: &str) -> ConfigError {
    ConfigError::Interpolate {
        path: path.clone(),
        message: message.into(),
    }
}

/// Expands the strings of fields marked `#[config(interpolate)]` in the context's root
/// schema (`HasRootConfigMeta`), reading variables from `HasEnvSnapshot`. Without a root
/// schema nothing is opted in and the tree is returned unchanged.
///
/// Reference paths (`${ref:...}`) are document paths, as written in files and env names.
pub struct InterpolateOptedIn;

#[cgp_impl(InterpolateOptedIn)]
impl<Config> Interpolator<Config>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanRaiseError<ConfigError>
        + HasRootConfigMeta
        + HasEnvSnapshot
        + HasLoadLimits,
{
    fn interpolate(&self, tree: Self::ConfigValue) -> Result<Self::ConfigValue, Self::Error> {
        let Some(meta) = self.root_config_meta() else {
            return Ok(tree);
        };
        interpolate_tree(tree, meta, self.env_snapshot(), &self.load_limits())
            .map_err(Self::raise_error)
    }
}

#[cfg(test)]
mod tests;
