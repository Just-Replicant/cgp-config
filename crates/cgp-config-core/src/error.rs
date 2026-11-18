//! [`ConfigError`], the one error type every built-in provider raises.
//!
//! Applications implement `CanRaiseError<ConfigError>` once (directly with
//! `ErrorTypeProviderComponent: UseType<ConfigError>` plus `RaiseFrom`, or through
//! `anyhow` / `eyre` `From`). Parser and IO errors are formatted into a variant at the
//! boundary where they occur, so a new format never adds an entry to existing wiring.
//!
//! `Display` is written by hand rather than derived with `#[cgp_auto_error]`: the derive
//! renders every field unconditionally, while these messages include optional parts
//! (span, override chain, "did you mean") and must suppress text on secret paths.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::{KeyPath, Meta, Origin, Span};

/// Stable `code()` strings, one per [`ConfigError`] variant. They are part of the semver
/// surface and index the error catalog.
pub mod codes {
    /// [`ConfigError::Io`](super::ConfigError::Io).
    pub const IO: &str = "config::io";
    /// [`ConfigError::Parse`](super::ConfigError::Parse).
    pub const PARSE: &str = "config::parse";
    /// [`ConfigError::Type`](super::ConfigError::Type).
    pub const TYPE: &str = "config::type";
    /// [`ConfigError::Missing`](super::ConfigError::Missing).
    pub const MISSING: &str = "config::missing";
    /// [`ConfigError::Unknown`](super::ConfigError::Unknown).
    pub const UNKNOWN: &str = "config::unknown";
    /// [`ConfigError::DuplicateKey`](super::ConfigError::DuplicateKey).
    pub const DUPLICATE_KEY: &str = "config::duplicate_key";
    /// [`ConfigError::MergeConflict`](super::ConfigError::MergeConflict).
    pub const MERGE_CONFLICT: &str = "config::merge_conflict";
    /// [`ConfigError::Validation`](super::ConfigError::Validation), and the default code
    /// of a [`ValidationIssue`](super::ValidationIssue).
    pub const VALIDATION: &str = "config::validation";
    /// [`ConfigError::Interpolate`](super::ConfigError::Interpolate).
    pub const INTERPOLATE: &str = "config::interpolate";
    /// [`ConfigError::InterpolateCycle`](super::ConfigError::InterpolateCycle).
    pub const INTERPOLATE_CYCLE: &str = "config::interpolate_cycle";
    /// [`ConfigError::AmbiguousEnv`](super::ConfigError::AmbiguousEnv).
    pub const AMBIGUOUS_ENV: &str = "config::ambiguous_env";
    /// [`ConfigError::EnvEncoding`](super::ConfigError::EnvEncoding).
    pub const ENV_ENCODING: &str = "config::env_encoding";
    /// [`ConfigError::Encoding`](super::ConfigError::Encoding).
    pub const ENCODING: &str = "config::encoding";
    /// [`ConfigError::SecretDenied`](super::ConfigError::SecretDenied).
    pub const SECRET_DENIED: &str = "config::secret_denied";
    /// [`ConfigError::SecretsDenied`](super::ConfigError::SecretsDenied).
    pub const SECRETS_DENIED: &str = "config::secrets_denied";
    /// [`ConfigError::Migration`](super::ConfigError::Migration).
    pub const MIGRATION: &str = "config::migration";
    /// [`ConfigError::LimitExceeded`](super::ConfigError::LimitExceeded).
    pub const LIMIT_EXCEEDED: &str = "config::limit_exceeded";
    /// [`ConfigError::ReloadRejected`](super::ConfigError::ReloadRejected).
    pub const RELOAD_REJECTED: &str = "config::reload_rejected";
    /// [`ConfigError::MissingProfile`](super::ConfigError::MissingProfile).
    pub const MISSING_PROFILE: &str = "config::missing_profile";
}

/// The fixed text shown instead of a message on a secret path.
pub(crate) const SECRET_MESSAGE: &str = "invalid secret value";

/// Everything that can go wrong while loading configuration.
///
/// Variants are `#[non_exhaustive]` as a whole: new variants are added in minor releases.
/// Match on [`code`](Self::code) for a stable identifier.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ConfigError {
    /// Reading a source failed. `message` is the formatted IO error; the core type does
    /// not hold `std::io::Error`, so it stays `no_std`.
    Io {
        /// The source being read.
        origin: Origin,
        /// The formatted cause.
        message: String,
    },
    /// The source text is not valid in its format.
    Parse {
        /// The source.
        origin: Origin,
        /// Where the parser stopped, when it reports spans.
        span: Option<Span>,
        /// The parser's message.
        message: String,
    },
    /// A value does not have the type the config struct expects.
    Type {
        /// The failing node.
        path: KeyPath,
        /// What was expected and found. Fixed text on secret paths.
        message: String,
        /// Provenance of the failing node.
        meta: Option<Box<Meta>>,
    },
    /// A required value or source is absent.
    Missing {
        /// The absent node.
        path: KeyPath,
        /// What was expected there (a type name or a description), possibly empty.
        expected: String,
    },
    /// Keys the config struct does not declare (with `DenyUnknownKeys`, or one key from a
    /// `#[serde(deny_unknown_fields)]` struct).
    ///
    /// `DenyUnknownKeys` reports every unknown key of the load at once, in document order,
    /// so an operator fixes all typos in one run. Never empty.
    Unknown {
        /// The unknown keys, at least one.
        keys: Vec<UnknownKey>,
    },
    /// A key appears twice in one document or one layer.
    DuplicateKey {
        /// The duplicated key.
        path: KeyPath,
        /// Provenance of the second occurrence.
        meta: Box<Meta>,
    },
    /// Two layers disagree on the shape of a node (a map over a scalar, an array over a
    /// map, ...). Never a silent replacement.
    MergeConflict {
        /// The conflicting node.
        path: KeyPath,
        /// Source of the lower layer's node.
        base: Origin,
        /// Source of the upper layer's node.
        overlay: Origin,
        /// Kind of the lower node (`map`, `array`, `string`, ...).
        base_kind: &'static str,
        /// Kind of the upper node.
        overlay_kind: &'static str,
    },
    /// One or more validators rejected the loaded config.
    Validation(ValidationErrors),
    /// Interpolating an opted-in string failed.
    Interpolate {
        /// The node being interpolated.
        path: KeyPath,
        /// Why.
        message: String,
    },
    /// Interpolation references form a cycle.
    InterpolateCycle {
        /// A node on the cycle.
        path: KeyPath,
    },
    /// An environment key maps to more than one config path.
    AmbiguousEnv {
        /// The environment variable name.
        key: String,
        /// Every path it could mean.
        candidates: Vec<KeyPath>,
    },
    /// An environment variable's value is not UTF-8. The bytes are not kept.
    EnvEncoding {
        /// The variable name.
        key: String,
    },
    /// A source is not UTF-8 (UTF-16 included).
    Encoding {
        /// The source.
        origin: Origin,
    },
    /// A secret reference points outside the allowlisted roots or schemes.
    SecretDenied {
        /// The reference, redacted.
        reference: RedactedRef,
    },
    /// The context does not resolve secrets (`DenySecrets`), but the config has some.
    SecretsDenied,
    /// A migration step failed (0.2).
    Migration {
        /// Version migrated from.
        from: u32,
        /// Version migrated to.
        to: u32,
        /// Why.
        message: String,
    },
    /// A [`LoadLimits`](crate::LoadLimits) bound was exceeded.
    LimitExceeded {
        /// The limit, by field name (`max_depth`, `max_keys`, ...).
        what: &'static str,
        /// Its configured value.
        limit: usize,
    },
    /// A reload changed a `reload = "restart"` field and was not applied (0.2).
    ReloadRejected {
        /// The restart-only paths that changed.
        paths: Vec<KeyPath>,
    },
    /// The active profile is not defined (with `RequireActiveProfile`).
    MissingProfile {
        /// The profile name chosen at bootstrap.
        profile: String,
    },
}

impl ConfigError {
    /// A read failure at `origin`.
    #[must_use]
    pub fn io(origin: Origin, message: impl Into<String>) -> Self {
        Self::Io {
            origin,
            message: message.into(),
        }
    }

    /// A syntax error at `origin`, optionally at `span`.
    #[must_use]
    pub fn parse(origin: Origin, span: Option<Span>, message: impl Into<String>) -> Self {
        Self::Parse {
            origin,
            span,
            message: message.into(),
        }
    }

    /// A type mismatch at `path`. When `meta` marks the node secret, `message` is
    /// discarded and replaced by a fixed text, so the raw input never reaches `Display`.
    #[must_use]
    pub fn type_error(
        path: impl Into<KeyPath>,
        message: impl Into<String>,
        meta: Option<Meta>,
    ) -> Self {
        let secret = meta.as_ref().is_some_and(|m| m.secret);
        Self::Type {
            path: path.into(),
            message: if secret {
                String::from(SECRET_MESSAGE)
            } else {
                message.into()
            },
            meta: meta.map(Box::new),
        }
    }

    /// A missing value at `path`.
    #[must_use]
    pub fn missing(path: impl Into<KeyPath>, expected: impl Into<String>) -> Self {
        Self::Missing {
            path: path.into(),
            expected: expected.into(),
        }
    }

    /// An unknown key at `path`.
    #[must_use]
    pub fn unknown(path: impl Into<KeyPath>, suggestion: Option<String>, meta: Meta) -> Self {
        Self::Unknown {
            keys: alloc::vec![UnknownKey::new(path, suggestion, meta)],
        }
    }

    /// Every key of `keys` as one error, or `None` when there are none.
    ///
    /// ```
    /// use cgp_config_core::{ConfigError, Meta, UnknownKey};
    ///
    /// let error = ConfigError::unknown_keys([
    ///     UnknownKey::new("http.prot", Some("port".into()), Meta::default()),
    ///     UnknownKey::new("nmae", Some("name".into()), Meta::default()),
    /// ])
    /// .unwrap();
    /// assert_eq!(error.code(), "config::unknown");
    /// assert_eq!(
    ///     error.to_string(),
    ///     r#"unknown keys (2): http.prot (did you mean "port"?); nmae (did you mean "name"?)"#
    /// );
    /// assert!(ConfigError::unknown_keys([]).is_none());
    /// ```
    #[must_use]
    pub fn unknown_keys(keys: impl IntoIterator<Item = UnknownKey>) -> Option<Self> {
        let keys: Vec<UnknownKey> = keys.into_iter().collect();
        if keys.is_empty() {
            None
        } else {
            Some(Self::Unknown { keys })
        }
    }

    /// A duplicate key at `path`.
    #[must_use]
    pub fn duplicate_key(path: impl Into<KeyPath>, meta: Meta) -> Self {
        Self::DuplicateKey {
            path: path.into(),
            meta: Box::new(meta),
        }
    }

    /// One validation failure with the default code `config::validation`.
    ///
    /// ```
    /// use cgp_config_core::ConfigError;
    ///
    /// let error = ConfigError::validation("http.port", "port is 0");
    /// assert_eq!(error.to_string(), "validation failed (1 error): http.port: port is 0");
    /// assert_eq!(error.code(), "config::validation");
    /// ```
    #[must_use]
    pub fn validation(path: impl Into<KeyPath>, message: impl Into<String>) -> Self {
        Self::Validation(ValidationErrors::from(ValidationIssue::new(path, message)))
    }

    /// A failed limit.
    #[must_use]
    pub fn limit_exceeded(what: &'static str, limit: usize) -> Self {
        Self::LimitExceeded { what, limit }
    }

    /// The stable code of this error, such as `config::type`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => codes::IO,
            Self::Parse { .. } => codes::PARSE,
            Self::Type { .. } => codes::TYPE,
            Self::Missing { .. } => codes::MISSING,
            Self::Unknown { .. } => codes::UNKNOWN,
            Self::DuplicateKey { .. } => codes::DUPLICATE_KEY,
            Self::MergeConflict { .. } => codes::MERGE_CONFLICT,
            Self::Validation(_) => codes::VALIDATION,
            Self::Interpolate { .. } => codes::INTERPOLATE,
            Self::InterpolateCycle { .. } => codes::INTERPOLATE_CYCLE,
            Self::AmbiguousEnv { .. } => codes::AMBIGUOUS_ENV,
            Self::EnvEncoding { .. } => codes::ENV_ENCODING,
            Self::Encoding { .. } => codes::ENCODING,
            Self::SecretDenied { .. } => codes::SECRET_DENIED,
            Self::SecretsDenied => codes::SECRETS_DENIED,
            Self::Migration { .. } => codes::MIGRATION,
            Self::LimitExceeded { .. } => codes::LIMIT_EXCEEDED,
            Self::ReloadRejected { .. } => codes::RELOAD_REJECTED,
            Self::MissingProfile { .. } => codes::MISSING_PROFILE,
        }
    }

    /// The key path the error is about, for variants that have one (the first key of an
    /// [`Unknown`](Self::Unknown) error).
    #[must_use]
    pub fn path(&self) -> Option<&KeyPath> {
        match self {
            Self::Type { path, .. }
            | Self::Missing { path, .. }
            | Self::DuplicateKey { path, .. }
            | Self::MergeConflict { path, .. }
            | Self::Interpolate { path, .. }
            | Self::InterpolateCycle { path } => Some(path),
            Self::Unknown { keys } => keys.first().map(|key| &key.path),
            _ => None,
        }
    }

    /// The node metadata, for variants that carry it (the first key of an
    /// [`Unknown`](Self::Unknown) error).
    #[must_use]
    pub fn meta(&self) -> Option<&Meta> {
        match self {
            Self::Type { meta, .. } => meta.as_deref(),
            Self::Unknown { keys } => keys.first().map(|key| &key.meta),
            Self::DuplicateKey { meta, .. } => Some(&**meta),
            _ => None,
        }
    }

    /// Prefixes the path of a path-carrying error with `prefix` (used when a section is
    /// deserialized or validated below its mount point).
    #[must_use]
    pub fn with_path_prefix(mut self, prefix: &KeyPath) -> Self {
        match &mut self {
            Self::Type { path, .. }
            | Self::Missing { path, .. }
            | Self::DuplicateKey { path, .. }
            | Self::MergeConflict { path, .. }
            | Self::Interpolate { path, .. }
            | Self::InterpolateCycle { path } => *path = prefix.concat(path),
            Self::Unknown { keys } => {
                for key in keys {
                    key.path = prefix.concat(&key.path);
                }
            }
            Self::Validation(errors) => {
                for item in &mut errors.items {
                    item.path = prefix.concat(&item.path);
                }
            }
            _ => {}
        }
        self
    }
}

fn write_paths(f: &mut fmt::Formatter<'_>, paths: &[KeyPath]) -> fmt::Result {
    for (index, path) in paths.iter().enumerate() {
        if index > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{path}")?;
    }
    Ok(())
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { origin, message } => write!(f, "{origin}: {message}"),
            Self::Parse {
                origin,
                span,
                message,
            } => match span {
                Some(span) => write!(f, "{origin}:{span}: syntax error: {message}"),
                None => write!(f, "{origin}: syntax error: {message}"),
            },
            Self::Type {
                path,
                message,
                meta,
            } => {
                let secret = meta.as_ref().is_some_and(|m| m.secret);
                let message = if secret { SECRET_MESSAGE } else { message };
                if path.is_root() {
                    f.write_str(message)?;
                } else {
                    write!(f, "{path}: {message}")?;
                }
                if let Some(meta) = meta {
                    if !meta.origin.is_unknown() {
                        write!(f, " at {}", meta.location())?;
                    }
                    if let Some(first) = meta.shadowed.first() {
                        write!(f, " (overrides {first})")?;
                    }
                }
                Ok(())
            }
            Self::Missing { path, expected } => {
                write!(f, "missing {path}")?;
                if !expected.is_empty() {
                    write!(f, " (expected {expected})")?;
                }
                Ok(())
            }
            Self::Unknown { keys } => match keys.as_slice() {
                [key] => {
                    write!(f, "unknown key {}", key.path)?;
                    if !key.meta.origin.is_unknown() {
                        write!(f, " at {}", key.meta.location())?;
                    }
                    if let Some(suggestion) = &key.suggestion {
                        write!(f, "; did you mean \"{suggestion}\"?")?;
                    }
                    Ok(())
                }
                keys => {
                    write!(f, "unknown keys ({})", keys.len())?;
                    for (index, key) in keys.iter().enumerate() {
                        f.write_str(if index == 0 { ": " } else { "; " })?;
                        write!(f, "{key}")?;
                    }
                    Ok(())
                }
            },
            Self::DuplicateKey { path, meta } => {
                write!(f, "duplicate key {path}")?;
                if !meta.origin.is_unknown() {
                    write!(f, " at {}", meta.location())?;
                }
                Ok(())
            }
            Self::MergeConflict {
                path,
                base,
                overlay,
                base_kind,
                overlay_kind,
            } => write!(
                f,
                "cannot merge {path}: {base_kind} from {base} vs {overlay_kind} from {overlay}"
            ),
            Self::Validation(errors) => {
                let count = errors.len();
                let noun = if count == 1 { "error" } else { "errors" };
                write!(f, "validation failed ({count} {noun})")?;
                if count > 0 {
                    write!(f, ": {errors}")?;
                }
                Ok(())
            }
            Self::Interpolate { path, message } => write!(f, "interpolation at {path}: {message}"),
            Self::InterpolateCycle { path } => write!(f, "interpolation cycle at {path}"),
            Self::AmbiguousEnv { key, candidates } => {
                write!(f, "ambiguous env key {key}")?;
                if !candidates.is_empty() {
                    f.write_str(" (matches ")?;
                    write_paths(f, candidates)?;
                    f.write_str(")")?;
                }
                Ok(())
            }
            Self::EnvEncoding { key } => write!(f, "env value for {key} is not valid UTF-8"),
            Self::Encoding { origin } => write!(f, "{origin}: not valid UTF-8"),
            Self::SecretDenied { reference } => write!(f, "secret reference rejected: {reference}"),
            Self::SecretsDenied => f.write_str("secrets are disabled"),
            Self::Migration { from, to, message } => {
                write!(f, "migration v{from}\u{2192}v{to}: {message}")
            }
            Self::LimitExceeded { what, limit } => write!(f, "limit exceeded: {what} ({limit})"),
            Self::ReloadRejected { paths } => {
                f.write_str("reload rejected; restart required")?;
                if !paths.is_empty() {
                    f.write_str(" (")?;
                    write_paths(f, paths)?;
                    f.write_str(")")?;
                }
                Ok(())
            }
            Self::MissingProfile { profile } => write!(f, "profile \"{profile}\" is not defined"),
        }
    }
}

impl core::error::Error for ConfigError {}

impl From<ValidationErrors> for ConfigError {
    fn from(errors: ValidationErrors) -> Self {
        Self::Validation(errors)
    }
}

/// One key the config struct does not declare.
///
/// `Display` writes `path at origin (did you mean "name"?)`, omitting the parts that are
/// unknown.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct UnknownKey {
    /// The key's path.
    pub path: KeyPath,
    /// The closest declared field name, when the key looks like a typo of it.
    pub suggestion: Option<String>,
    /// The key's provenance.
    pub meta: Meta,
}

impl UnknownKey {
    /// An unknown key.
    #[must_use]
    pub fn new(path: impl Into<KeyPath>, suggestion: Option<String>, meta: Meta) -> Self {
        Self {
            path: path.into(),
            suggestion,
            meta,
        }
    }
}

impl fmt::Display for UnknownKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path)?;
        if !self.meta.origin.is_unknown() {
            write!(f, " at {}", self.meta.location())?;
        }
        if let Some(suggestion) = &self.suggestion {
            write!(f, " (did you mean \"{suggestion}\"?)")?;
        }
        Ok(())
    }
}

/// One validation failure.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ValidationIssue {
    /// The rejected node.
    pub path: KeyPath,
    /// Stable code, `config::validation` unless the validator chose another.
    pub code: &'static str,
    /// Human-readable reason. Validators must not put secret values here.
    pub message: String,
    /// Provenance of the rejected node, when known.
    pub meta: Option<Meta>,
}

impl ValidationIssue {
    /// An issue with the default code.
    #[must_use]
    pub fn new(path: impl Into<KeyPath>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            code: codes::VALIDATION,
            message: message.into(),
            meta: None,
        }
    }

    /// Replaces the code.
    #[must_use]
    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = code;
        self
    }

    /// Attaches provenance.
    #[must_use]
    pub fn with_meta(mut self, meta: Meta) -> Self {
        self.meta = Some(meta);
        self
    }
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// Every validation failure of one load, in the order validators reported them.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct ValidationErrors {
    /// The failures.
    pub items: Vec<ValidationIssue>,
}

impl ValidationErrors {
    /// No failures.
    #[must_use]
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Adds one failure.
    pub fn push(&mut self, issue: ValidationIssue) {
        self.items.push(issue);
    }

    /// Number of failures.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no failures.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iterates over the failures.
    pub fn iter(&self) -> core::slice::Iter<'_, ValidationIssue> {
        self.items.iter()
    }

    /// `Ok(())` when empty, otherwise `Err(ConfigError::Validation(self))`.
    ///
    /// # Errors
    ///
    /// When at least one failure was recorded.
    pub fn into_result(self) -> Result<(), ConfigError> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Validation(self))
        }
    }
}

impl From<ValidationIssue> for ValidationErrors {
    fn from(issue: ValidationIssue) -> Self {
        Self {
            items: alloc::vec![issue],
        }
    }
}

impl Extend<ValidationIssue> for ValidationErrors {
    fn extend<I: IntoIterator<Item = ValidationIssue>>(&mut self, iter: I) {
        self.items.extend(iter);
    }
}

impl FromIterator<ValidationIssue> for ValidationErrors {
    fn from_iter<I: IntoIterator<Item = ValidationIssue>>(iter: I) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }
}

impl<'a> IntoIterator for &'a ValidationErrors {
    type Item = &'a ValidationIssue;
    type IntoIter = core::slice::Iter<'a, ValidationIssue>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl fmt::Display for ValidationErrors {
    /// The failures joined with `; `.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, item) in self.items.iter().enumerate() {
            if index > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{item}")?;
        }
        Ok(())
    }
}

/// A secret reference with the sensitive part removed, safe to display and log.
///
/// `Display` shows the scheme and either an allowlisted relative name (for `file:` and
/// `docker:` references whose name is not itself sensitive) or an FNV-1a digest of the
/// rest of the reference. The raw reference is not stored.
///
/// ```
/// use cgp_config_core::RedactedRef;
///
/// let shown = RedactedRef::new("vault:kv/app?token=hunter2").to_string();
/// assert!(shown.starts_with("vault:<redacted #"));
/// assert!(!shown.contains("hunter2"));
/// assert_eq!(RedactedRef::with_name("file", "db_password").to_string(), "file:db_password");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RedactedRef {
    scheme: String,
    shown: Shown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Shown {
    Digest(u64),
    Name(String),
}

impl RedactedRef {
    /// Redacts `reference` (`scheme:rest`), keeping the scheme and a digest of the rest.
    /// A reference without `:` has the scheme `unknown`.
    #[must_use]
    pub fn new(reference: &str) -> Self {
        let (scheme, rest) = reference.split_once(':').unwrap_or(("unknown", reference));
        Self {
            scheme: String::from(scheme),
            shown: Shown::Digest(fnv1a64(rest.as_bytes())),
        }
    }

    /// A reference whose name is safe to show (an allowlisted relative file name).
    #[must_use]
    pub fn with_name(scheme: &str, name: &str) -> Self {
        Self {
            scheme: String::from(scheme),
            shown: Shown::Name(String::from(name)),
        }
    }

    /// The scheme (`env`, `file`, `docker`, ...).
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.scheme
    }
}

impl fmt::Display for RedactedRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.shown {
            Shown::Digest(digest) => write!(f, "{}:<redacted #{digest:016x}>", self.scheme),
            Shown::Name(name) => write!(f, "{}:{name}", self.scheme),
        }
    }
}

/// FNV-1a, 64-bit. Stable across Rust releases (unlike `DefaultHasher`).
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}
