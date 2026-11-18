use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::{KeyPath, Meta, Origin};

/// Something worth telling the operator that does not fail the load.
///
/// Warnings name keys and origins, never values.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum LoadWarning {
    /// A file had no `$version`; it is treated as version 1.
    AssumedVersion {
        /// The file.
        origin: Origin,
    },
    /// A file declared `$version`.
    DocumentVersion {
        /// The file.
        origin: Origin,
        /// The declared version.
        version: u32,
    },
    /// A key the config struct does not declare (with `WarnUnknownKeys`).
    UnknownKey {
        /// The key.
        path: KeyPath,
        /// The closest declared name, when it looks like a typo.
        suggestion: Option<String>,
        /// Provenance of the key.
        meta: Option<Meta>,
    },
    /// A config file is group- or world-writable (Unix).
    WorldWritable {
        /// The file.
        origin: Origin,
    },
    /// A file with a secret reference is group- or world-readable (Unix).
    SecretWorldReadable {
        /// The file.
        origin: Origin,
    },
    /// The active profile has no `$profiles` table and no profile file.
    MissingProfile {
        /// The profile name.
        profile: String,
    },
    /// A `$include` key was stripped (includes load from 0.2).
    IgnoredInclude {
        /// The file.
        origin: Origin,
    },
    /// A deprecated alias was used.
    DeprecatedKey {
        /// The current path.
        path: KeyPath,
        /// The alias that was found.
        alias: String,
        /// The deprecation note.
        note: String,
    },
}

impl LoadWarning {
    /// Stable code for the warning, such as `config::warn::unknown_key`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::AssumedVersion { .. } => "config::warn::assumed_version",
            Self::DocumentVersion { .. } => "config::warn::document_version",
            Self::UnknownKey { .. } => "config::warn::unknown_key",
            Self::WorldWritable { .. } => "config::warn::world_writable",
            Self::SecretWorldReadable { .. } => "config::warn::secret_world_readable",
            Self::MissingProfile { .. } => "config::warn::missing_profile",
            Self::IgnoredInclude { .. } => "config::warn::ignored_include",
            Self::DeprecatedKey { .. } => "config::warn::deprecated_key",
        }
    }
}

impl fmt::Display for LoadWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssumedVersion { origin } => {
                write!(f, "{origin}: no $version; assuming version 1")
            }
            Self::DocumentVersion { origin, version } => {
                write!(f, "{origin}: document version {version}")
            }
            Self::UnknownKey {
                path,
                suggestion,
                meta,
            } => {
                write!(f, "unknown key {path}")?;
                if let Some(meta) = meta.as_ref().filter(|m| !m.origin.is_unknown()) {
                    write!(f, " at {}", meta.location())?;
                }
                if let Some(suggestion) = suggestion {
                    write!(f, "; did you mean \"{suggestion}\"?")?;
                }
                Ok(())
            }
            Self::WorldWritable { origin } => {
                write!(f, "{origin}: file is writable by group or others")
            }
            Self::SecretWorldReadable { origin } => write!(
                f,
                "{origin}: file contains a secret reference and is readable by group or others"
            ),
            Self::MissingProfile { profile } => {
                write!(
                    f,
                    "profile \"{profile}\" is not defined; using the default profile"
                )
            }
            Self::IgnoredInclude { origin } => {
                write!(f, "{origin}: $include is not supported before 0.2; ignored")
            }
            Self::DeprecatedKey { path, alias, note } => {
                write!(f, "deprecated key {alias} (now {path})")?;
                if !note.is_empty() {
                    write!(f, ": {note}")?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(feature = "std")]
type Lock<T> = std::sync::Mutex<T>;
#[cfg(not(feature = "std"))]
type Lock<T> = spin::Mutex<T>;

/// The warnings of a load, stored on the loader context.
///
/// Providers record into it through `&self` (the pipeline's signatures take `&self`), so
/// the warnings sit behind a lock: `std::sync::Mutex` with the `std` feature, a spin lock
/// otherwise. The report is `Send + Sync` either way.
///
/// ```
/// use cgp_config_core::{LoadReport, LoadWarning, Origin};
///
/// let report = LoadReport::new();
/// report.push(LoadWarning::AssumedVersion { origin: Origin::file("app.toml") });
/// assert_eq!(report.len(), 1);
/// let warnings = report.take();
/// assert_eq!(warnings[0].code(), "config::warn::assumed_version");
/// assert!(report.is_empty());
/// ```
#[derive(Default)]
pub struct LoadReport {
    warnings: Lock<Vec<LoadWarning>>,
}

impl LoadReport {
    /// An empty report.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut Vec<LoadWarning>) -> R) -> R {
        #[cfg(feature = "std")]
        let mut guard = self
            .warnings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(not(feature = "std"))]
        let mut guard = self.warnings.lock();
        f(&mut guard)
    }

    /// Records a warning.
    pub fn push(&self, warning: LoadWarning) {
        self.with(|warnings| warnings.push(warning));
    }

    /// A copy of the recorded warnings.
    #[must_use]
    pub fn warnings(&self) -> Vec<LoadWarning> {
        self.with(|warnings| warnings.clone())
    }

    /// Removes and returns the recorded warnings.
    #[must_use]
    pub fn take(&self) -> Vec<LoadWarning> {
        self.with(core::mem::take)
    }

    /// Discards the recorded warnings (a loader does this before each load).
    pub fn clear(&self) {
        self.with(Vec::clear);
    }

    /// Number of recorded warnings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.with(|warnings| warnings.len())
    }

    /// Whether no warning was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Clone for LoadReport {
    fn clone(&self) -> Self {
        Self {
            warnings: Lock::new(self.warnings()),
        }
    }
}

impl fmt::Debug for LoadReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadReport")
            .field("warnings", &self.warnings())
            .finish()
    }
}

impl PartialEq for LoadReport {
    fn eq(&self, other: &Self) -> bool {
        self.warnings() == other.warnings()
    }
}

impl From<Vec<LoadWarning>> for LoadReport {
    fn from(warnings: Vec<LoadWarning>) -> Self {
        Self {
            warnings: Lock::new(warnings),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::{String, ToString};
    use alloc::vec;

    use super::{LoadReport, LoadWarning};
    use crate::{KeyPath, Meta, Origin};

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn report_is_send_sync_and_records() {
        assert_send_sync::<LoadReport>();
        let report = LoadReport::from(vec![LoadWarning::MissingProfile {
            profile: "prod".into(),
        }]);
        let copy = report.clone();
        assert_eq!(copy, report);
        report.push(LoadWarning::IgnoredInclude {
            origin: Origin::file("a.toml"),
        });
        assert_eq!(report.warnings().len(), 2);
        assert!(alloc::format!("{report:?}").contains("IgnoredInclude"));
        report.clear();
        assert!(report.is_empty());
        assert_eq!(copy.len(), 1);
    }

    #[test]
    fn warning_text_and_codes() {
        let file = || Origin::file("app.toml");
        let cases = [
            (
                LoadWarning::AssumedVersion { origin: file() },
                "app.toml: no $version; assuming version 1",
                "config::warn::assumed_version",
            ),
            (
                LoadWarning::DocumentVersion {
                    origin: file(),
                    version: 2,
                },
                "app.toml: document version 2",
                "config::warn::document_version",
            ),
            (
                LoadWarning::UnknownKey {
                    path: KeyPath::from("http.prot"),
                    suggestion: Some("port".into()),
                    meta: Some(Meta::new(file())),
                },
                "unknown key http.prot at app.toml; did you mean \"port\"?",
                "config::warn::unknown_key",
            ),
            (
                LoadWarning::UnknownKey {
                    path: KeyPath::from("x"),
                    suggestion: None,
                    meta: Some(Meta::default()),
                },
                "unknown key x",
                "config::warn::unknown_key",
            ),
            (
                LoadWarning::WorldWritable { origin: file() },
                "app.toml: file is writable by group or others",
                "config::warn::world_writable",
            ),
            (
                LoadWarning::SecretWorldReadable { origin: file() },
                "app.toml: file contains a secret reference and is readable by group or others",
                "config::warn::secret_world_readable",
            ),
            (
                LoadWarning::MissingProfile {
                    profile: "prod".into(),
                },
                "profile \"prod\" is not defined; using the default profile",
                "config::warn::missing_profile",
            ),
            (
                LoadWarning::IgnoredInclude { origin: file() },
                "app.toml: $include is not supported before 0.2; ignored",
                "config::warn::ignored_include",
            ),
            (
                LoadWarning::DeprecatedKey {
                    path: KeyPath::from("http.port"),
                    alias: "listen_port".into(),
                    note: "use http.port".into(),
                },
                "deprecated key listen_port (now http.port): use http.port",
                "config::warn::deprecated_key",
            ),
            (
                LoadWarning::DeprecatedKey {
                    path: KeyPath::from("a"),
                    alias: "b".into(),
                    note: String::new(),
                },
                "deprecated key b (now a)",
                "config::warn::deprecated_key",
            ),
        ];
        for (warning, text, code) in cases {
            assert_eq!(warning.to_string(), text);
            assert_eq!(warning.code(), code);
        }
    }
}
