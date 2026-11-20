//! Where the main config file is: [`ConfigLocation`] and the search order of
//! [`SearchPath`].

use std::path::{Path, PathBuf};

use cgp_config_core::EnvSnapshot;
use cgp_config_formats::FormatKind;
use cgp_fork::prelude::*;

/// The resolved main config file, decided once at bootstrap.
///
/// The sibling files (`{stem}.{profile}.{ext}`, `{stem}.local.{ext}`, `conf.d/`) are
/// derived from it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigLocation {
    /// The file, when one was given or found.
    pub file: Option<PathBuf>,
    /// Whether a missing `file` fails the load (an explicit path), rather than leaving
    /// the layer absent.
    pub required: bool,
    /// Every candidate the search looked at, for the `Missing` error.
    pub searched: Vec<PathBuf>,
}

impl ConfigLocation {
    /// No config file: file layers are absent.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// An explicit file, required to exist.
    #[must_use]
    pub fn explicit(path: impl Into<PathBuf>) -> Self {
        Self {
            file: Some(path.into()),
            required: true,
            searched: Vec::new(),
        }
    }

    /// An optional file (absent is not an error).
    #[must_use]
    pub fn optional(path: impl Into<PathBuf>) -> Self {
        Self {
            file: Some(path.into()),
            required: false,
            searched: Vec::new(),
        }
    }

    /// Makes a missing file an error (`ConfigError::Missing` listing the searched paths).
    #[must_use]
    pub fn require(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// The directory of the file.
    #[must_use]
    pub fn dir(&self) -> Option<&Path> {
        self.file
            .as_deref()
            .map(|file| file.parent().unwrap_or_else(|| Path::new("")))
    }

    /// The file name without its last extension (`app` for `app.toml`).
    #[must_use]
    pub fn stem(&self) -> Option<&str> {
        self.file.as_deref()?.file_stem()?.to_str()
    }

    /// The last extension (`toml`).
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.file.as_deref()?.extension()?.to_str()
    }

    /// `{dir}/{stem}.{suffix}.{ext}`: the profile (`suffix` = profile name) and local
    /// (`suffix` = `local`) siblings.
    #[must_use]
    pub fn sibling(&self, suffix: &str) -> Option<PathBuf> {
        let name = format!("{}.{suffix}.{}", self.stem()?, self.extension()?);
        Some(self.dir()?.join(name))
    }

    /// `{dir}/conf.d`.
    #[must_use]
    pub fn conf_dir(&self) -> Option<PathBuf> {
        Some(self.dir()?.join("conf.d"))
    }
}

/// The main config file location of a context. Implemented automatically for a context
/// with a `config_location: ConfigLocation` field.
#[cgp_auto_getter]
pub trait HasConfigLocation {
    /// The location.
    fn config_location(&self) -> &ConfigLocation;
}

/// The first directory of a `CONFIGURATION_DIRECTORY` value. systemd (Unix only) joins
/// several directories with `:`, so `unix_list` splits there and skips empty entries;
/// with `unix_list` false (any other platform) the whole value is one path, which keeps
/// `C:\\ProgramData\\svc` intact.
///
/// ```
/// use std::path::PathBuf;
/// use cgp_config_providers::location::first_configuration_directory;
///
/// assert_eq!(
///     first_configuration_directory(":/etc/svc:/etc/other", true),
///     Some(PathBuf::from("/etc/svc"))
/// );
/// assert_eq!(
///     first_configuration_directory(r"C:\ProgramData\svc", false),
///     Some(PathBuf::from(r"C:\ProgramData\svc"))
/// );
/// assert_eq!(first_configuration_directory("", false), None);
/// ```
#[must_use]
pub fn first_configuration_directory(value: &str, unix_list: bool) -> Option<PathBuf> {
    if unix_list {
        value
            .split(':')
            .find(|dir| !dir.is_empty())
            .map(PathBuf::from)
    } else {
        (!value.is_empty()).then(|| PathBuf::from(value))
    }
}

/// The search order for the main config file (design plan §6.3):
///
/// 1. `{CONFIGURATION_DIRECTORY}/{app}.{ext}` when systemd set `CONFIGURATION_DIRECTORY`
///    (only its first entry: on Unix systemd lists several separated by `:`; elsewhere
///    the value is one path, so a Windows `C:\\…` is not split at its drive colon, see
///    [`first_configuration_directory`]);
/// 2. `./{app}.{ext}`, `./config.{ext}`, `./config/{app}.{ext}` in the working directory;
/// 3. the same three names in the directory of the executable;
/// 4. `{project_config_dir}/{app}.{ext}` (`directories::ProjectDirs`).
///
/// `{ext}` runs over the enabled formats, TOML first. The first existing file wins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchPath {
    app: String,
    configuration_directory: Option<PathBuf>,
    working_dir: Option<PathBuf>,
    exe_dir: Option<PathBuf>,
    project_dir: Option<PathBuf>,
}

impl SearchPath {
    /// A search for `app` with no directories yet.
    #[must_use]
    pub fn new(app: impl Into<String>) -> Self {
        Self {
            app: app.into(),
            ..Self::default()
        }
    }

    /// The standard directories: `CONFIGURATION_DIRECTORY` from `env`, the working
    /// directory, the executable's directory, and the platform's project config
    /// directory.
    #[must_use]
    pub fn standard(app: impl Into<String>, env: &EnvSnapshot) -> Self {
        let app = app.into();
        let project_dir = directories::ProjectDirs::from("", "", &app)
            .map(|dirs| dirs.config_dir().to_path_buf());
        Self {
            configuration_directory: env
                .get("CONFIGURATION_DIRECTORY")
                .and_then(|dirs| first_configuration_directory(dirs, cfg!(unix))),
            working_dir: std::env::current_dir().ok(),
            exe_dir: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf)),
            project_dir,
            app,
        }
    }

    /// Sets `CONFIGURATION_DIRECTORY`.
    #[must_use]
    pub fn with_configuration_directory(mut self, dir: Option<PathBuf>) -> Self {
        self.configuration_directory = dir;
        self
    }

    /// Sets the working directory.
    #[must_use]
    pub fn with_working_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.working_dir = dir;
        self
    }

    /// Sets the executable's directory.
    #[must_use]
    pub fn with_exe_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.exe_dir = dir;
        self
    }

    /// Sets the project config directory.
    #[must_use]
    pub fn with_project_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.project_dir = dir;
        self
    }

    /// Every candidate, in order.
    #[must_use]
    pub fn candidates(&self) -> Vec<PathBuf> {
        let extensions: Vec<&str> = FormatKind::enabled()
            .flat_map(|kind| kind.extensions().iter().copied())
            .collect();
        let app = self.app.as_str();
        let mut out = Vec::new();
        let mut push_names = |dir: &Path, names: &[&dyn Fn(&str) -> PathBuf]| {
            for name in names {
                for ext in &extensions {
                    out.push(dir.join(name(ext)));
                }
            }
        };
        let own = |ext: &str| PathBuf::from(format!("{app}.{ext}"));
        let generic = |ext: &str| PathBuf::from(format!("config.{ext}"));
        let nested = |ext: &str| Path::new("config").join(format!("{app}.{ext}"));
        if let Some(dir) = &self.configuration_directory {
            push_names(dir, &[&own]);
        }
        for dir in [&self.working_dir, &self.exe_dir].into_iter().flatten() {
            push_names(dir, &[&own, &generic, &nested]);
        }
        if let Some(dir) = &self.project_dir {
            push_names(dir, &[&own]);
        }
        let mut seen = std::collections::BTreeSet::new();
        out.retain(|path| seen.insert(path.clone()));
        out
    }

    /// The first candidate for which `exists` holds, as an optional location that
    /// remembers every candidate.
    pub fn resolve(&self, exists: impl Fn(&Path) -> bool) -> ConfigLocation {
        let searched = self.candidates();
        let file = searched.iter().find(|path| exists(path)).cloned();
        ConfigLocation {
            file,
            required: false,
            searched,
        }
    }
}

/// Whether a file name is an editor scratch file (`*~`, `*.swp`, `*.tmp`, `.#*`), which
/// is never a config file.
#[must_use]
pub fn is_scratch_name(name: &str) -> bool {
    name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".tmp")
        || name.starts_with(".#")
}

#[cfg(test)]
mod tests {
    // Some tests need a format to search for.
    #![cfg_attr(not(feature = "toml"), allow(unused_imports))]

    use std::path::{Path, PathBuf};

    use super::{ConfigLocation, SearchPath, is_scratch_name};

    #[test]
    #[cfg(feature = "toml")]
    fn configuration_directory_comes_first() {
        let search = SearchPath::new("svc")
            .with_configuration_directory(Some("/etc/svc".into()))
            .with_working_dir(Some("/work".into()))
            .with_exe_dir(Some("/opt/svc/bin".into()))
            .with_project_dir(Some("/home/u/.config/svc".into()));
        let all = search.candidates();
        // Other enabled formats interleave (`svc.toml`, `svc.json`, ...); TOML comes first.
        let candidates: Vec<_> = all
            .iter()
            .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
            .cloned()
            .collect();
        assert_eq!(all[0], candidates[0]);
        assert_eq!(candidates[0], PathBuf::from("/etc/svc/svc.toml"));
        assert_eq!(candidates[1], PathBuf::from("/work/svc.toml"));
        assert_eq!(candidates[2], PathBuf::from("/work/config.toml"));
        assert_eq!(candidates[3], PathBuf::from("/work/config/svc.toml"));
        assert_eq!(candidates[4], PathBuf::from("/opt/svc/bin/svc.toml"));
        assert_eq!(
            candidates.last().unwrap(),
            &PathBuf::from("/home/u/.config/svc/svc.toml")
        );

        let both = |p: &Path| p.starts_with("/etc/svc") || p.starts_with("/work");
        assert_eq!(search.resolve(both).file, Some("/etc/svc/svc.toml".into()));
        let no_systemd = search.clone().with_configuration_directory(None);
        assert_eq!(no_systemd.resolve(both).file, Some("/work/svc.toml".into()));
        let none = search.resolve(|_| false);
        assert_eq!(none.file, None);
        assert_eq!(none.searched.len(), all.len());
    }

    #[test]
    fn siblings_follow_the_base_file() {
        let location = ConfigLocation::explicit("/etc/app/app.toml");
        assert_eq!(
            location.sibling("prod"),
            Some("/etc/app/app.prod.toml".into())
        );
        assert_eq!(
            location.sibling("local"),
            Some("/etc/app/app.local.toml".into())
        );
        assert_eq!(location.conf_dir(), Some("/etc/app/conf.d".into()));
        let relative = ConfigLocation::optional("app.json");
        assert_eq!(relative.dir(), Some(Path::new("")));
        assert_eq!(relative.sibling("dev"), Some("app.dev.json".into()));
        assert_eq!(ConfigLocation::none().sibling("x"), None);
        assert!(!relative.require(false).required);
    }

    #[test]
    fn scratch_names() {
        for name in ["a.toml~", "a.toml.swp", "a.tmp", ".#a.toml"] {
            assert!(is_scratch_name(name), "{name}");
        }
        assert!(!is_scratch_name("10-db.toml"));
    }
}
