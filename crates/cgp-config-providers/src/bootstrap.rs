//! [`Bootstrap`]: the settings decided before any config is loaded (design plan §6.2).
//!
//! | Setting | CLI | Env | Built-in |
//! |---|---|---|---|
//! | config path | `--config` | `{PREFIX}_CONFIG` | the search order of [`SearchPath`] |
//! | profile | `--profile` | `{PREFIX}_PROFILE` | `"default"` |
//! | JSON document | — | `{PREFIX}_DOCUMENT` | absent |
//! | env prefix | `--env-prefix` | — | the app name, uppercased (`my-app` → `MY_APP`) |
//!
//! CLI wins over env, env over built-in. The three bootstrap variables are removed from
//! the snapshot afterwards, so they never become config values. The process environment
//! is read once ([`Bootstrap::capture_env`]); nothing reads it again or writes it.
//!
//! Bootstrap knows the loader's layer list once [`Bootstrap::layers`] names it (the
//! `layers` aliases, e.g. `TwelveFactorCliLayers`):
//!
//! * `--set` is accepted only when the list has `CliArgs`; otherwise
//!   [`Bootstrap::from_args`] fails instead of collecting overrides no layer reads;
//! * the `.env` path defaults to `.env` in the working directory when the list has
//!   `DotEnv` (an explicit [`Bootstrap::dotenv_path`] wins, `None` disables it).
//!
//! [`StdLoader`](crate::loader::StdLoader) checks the same against its own list
//! ([`Bootstrapped::check_layers`]), so overrides, a `{PREFIX}_DOCUMENT`, or a `.env`
//! path that its layers would not read are an error, never silently dropped.

use std::ffi::OsString;
use std::path::PathBuf;

use cgp_config_core::markers::{CliArgs, DotEnv, EnvDocument};
use cgp_config_core::{ConfigError, EnvSnapshot, Origin};

use crate::layers::LayerList;
use crate::location::{ConfigLocation, SearchPath};
use crate::sources::CliOverrides;

/// Bootstrap inputs. See the [module docs](self).
#[derive(Debug, Clone, Default)]
pub struct Bootstrap {
    app: String,
    prefix: Option<String>,
    env: EnvSnapshot,
    config_path: Option<PathBuf>,
    profile: Option<String>,
    search: Option<SearchPath>,
    require_file: bool,
    /// `None`: not set (default by layer list); `Some(None)`: disabled.
    dotenv_path: Option<Option<PathBuf>>,
    cli_overrides: CliOverrides,
    accepts_cli: bool,
    uses_dotenv: bool,
}

/// The outcome of [`Bootstrap::finish`]: everything a loader context needs.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Bootstrapped {
    /// The env prefix (`APP`).
    pub env_prefix: String,
    /// The snapshot without the bootstrap variables.
    pub env_snapshot: EnvSnapshot,
    /// The active profile.
    pub active_profile: String,
    /// The main config file.
    pub config_location: ConfigLocation,
    /// The `{PREFIX}_DOCUMENT` text.
    pub env_document: Option<String>,
    /// The `.env` file (feature `dotenv`): the explicit path, or `.env` in the working
    /// directory when the layer list has `DotEnv`.
    pub dotenv_path: Option<PathBuf>,
    /// Whether `dotenv_path` was set (or disabled) with [`Bootstrap::dotenv_path`] rather
    /// than defaulted.
    pub dotenv_explicit: bool,
    /// Command-line `--set` overrides.
    pub cli_overrides: CliOverrides,
}

/// The default `.env` file: `.env` in the working directory.
pub const DEFAULT_DOTENV: &str = ".env";

impl Bootstrapped {
    /// Fails when a bootstrap input would be read by no layer of `Layers`: `--set`
    /// overrides without `CliArgs`, a `{PREFIX}_DOCUMENT` without `EnvDocument`, or an
    /// explicit `.env` path without `DotEnv`. Fills in the default `.env` path when
    /// `Layers` has `DotEnv` and none was chosen.
    ///
    /// # Errors
    ///
    /// `ConfigError::Parse` naming the input and the missing layer.
    ///
    /// ```
    /// use cgp_config_providers::bootstrap::Bootstrap;
    /// use cgp_config_providers::layers::{TwelveFactorCliLayers, TwelveFactorLayers};
    /// use cgp_config_providers::sources::CliOverrides;
    ///
    /// let boot = Bootstrap::new("app")
    ///     .cli_overrides(CliOverrides::new().with("http.port", "1"))
    ///     .finish_with(|_| false);
    /// assert!(boot.clone().check_layers::<TwelveFactorLayers>().is_err());
    /// assert!(boot.check_layers::<TwelveFactorCliLayers>().is_ok());
    /// ```
    pub fn check_layers<Layers: LayerList>(mut self) -> Result<Self, ConfigError> {
        if !self.cli_overrides.is_empty() && !Layers::contains::<CliArgs>() {
            return Err(ConfigError::parse(
                Origin::cli("--set"),
                None,
                "`--set` overrides were given, but the loader has no `CliArgs` layer; use a \
                 layer list with `CliArgs` (e.g. `TwelveFactorCliLayers`)",
            ));
        }
        if self.env_document.is_some() && !Layers::contains::<EnvDocument>() {
            let name = format!("{}_DOCUMENT", self.env_prefix);
            return Err(ConfigError::parse(
                Origin::env_document(name.clone()),
                None,
                format!(
                    "`{name}` is set, but the loader has no `EnvDocument` layer; unset it or \
                     use a layer list with `EnvDocument`"
                ),
            ));
        }
        if Layers::contains::<DotEnv>() {
            if !self.dotenv_explicit && self.dotenv_path.is_none() {
                self.dotenv_path = Some(PathBuf::from(DEFAULT_DOTENV));
            }
        } else if self.dotenv_explicit && self.dotenv_path.is_some() {
            return Err(ConfigError::parse(
                Origin::file(
                    self.dotenv_path
                        .as_deref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default(),
                ),
                None,
                "a `.env` path was set, but the loader has no `DotEnv` layer",
            ));
        }
        Ok(self)
    }
}

/// The default env prefix of an app name: uppercase, `-` and `.` as `_`.
///
/// ```
/// assert_eq!(cgp_config_providers::bootstrap::default_prefix("my-app"), "MY_APP");
/// ```
#[must_use]
pub fn default_prefix(app: &str) -> String {
    app.chars()
        .map(|c| match c {
            '-' | '.' | ' ' => '_',
            c => c.to_ascii_uppercase(),
        })
        .collect()
}

impl Bootstrap {
    /// A bootstrap for `app` (the config file stem and the default prefix), with an empty
    /// environment.
    #[must_use]
    pub fn new(app: impl Into<String>) -> Self {
        Self {
            app: app.into(),
            ..Self::default()
        }
    }

    /// Captures the process environment into the snapshot (the only place cgp-config
    /// reads it).
    ///
    /// # Errors
    ///
    /// `ConfigError::EnvEncoding` for a non-UTF-8 variable under the prefix (or one of
    /// the bootstrap names); other non-UTF-8 variables are skipped.
    pub fn capture_env(self) -> Result<Self, ConfigError> {
        self.capture_env_from(std::env::vars_os())
    }

    /// Captures `vars` as if they were the process environment.
    ///
    /// A variable that is not valid UTF-8 is an error only when it is one cgp-config would
    /// read: `{PREFIX}__…` (the env layer), `{PREFIX}_CONFIG`, `{PREFIX}_PROFILE`,
    /// `{PREFIX}_DOCUMENT`, or `CONFIGURATION_DIRECTORY`. Others are skipped, so
    /// `APPLE=…` does not fail an app with prefix `APP`. Set the prefix
    /// ([`env_prefix`](Self::env_prefix)) before capturing.
    ///
    /// # Errors
    ///
    /// As [`capture_env`](Self::capture_env).
    pub fn capture_env_from(
        mut self,
        vars: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Result<Self, ConfigError> {
        let prefix = self.prefix();
        for (key, value) in vars {
            match (key.into_string(), value.into_string()) {
                (Ok(key), Ok(value)) => self.env.insert(key, value),
                (Ok(key), Err(_)) if is_read_variable(&key, &prefix) => {
                    return Err(ConfigError::EnvEncoding { key });
                }
                (Err(key), _) if is_read_variable(&key.to_string_lossy(), &prefix) => {
                    return Err(ConfigError::EnvEncoding {
                        key: key.to_string_lossy().into_owned(),
                    });
                }
                _ => {}
            }
        }
        Ok(self)
    }

    /// Whether `--set` overrides are accepted (the layer list has `CliArgs`).
    #[must_use]
    pub fn accepts_cli_overrides(&self) -> bool {
        self.accepts_cli
    }

    /// Names the loader's layer list: `--set` is accepted only when it has `CliArgs`,
    /// and the `.env` path defaults to [`DEFAULT_DOTENV`] when it has `DotEnv`.
    #[must_use]
    pub fn layers<Layers: LayerList>(mut self) -> Self {
        self.accepts_cli = Layers::contains::<CliArgs>();
        self.uses_dotenv = Layers::contains::<DotEnv>();
        self
    }

    /// Uses `env` as the snapshot (tests, embedding).
    #[must_use]
    pub fn with_env(mut self, env: EnvSnapshot) -> Self {
        self.env = env;
        self
    }

    /// Sets the env prefix (`--env-prefix`).
    #[must_use]
    pub fn env_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Sets the config path (`--config`): it must exist.
    #[must_use]
    pub fn config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = Some(path.into());
        self
    }

    /// Sets the profile (`--profile`).
    #[must_use]
    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// Replaces the search order (default: [`SearchPath::standard`]).
    #[must_use]
    pub fn search(mut self, search: SearchPath) -> Self {
        self.search = Some(search);
        self
    }

    /// Makes "no config file found" an error (`ConfigError::Missing` listing the searched
    /// paths). By default the file layers are simply absent, so a service can run on env
    /// alone.
    #[must_use]
    pub fn require_file(mut self, required: bool) -> Self {
        self.require_file = required;
        self
    }

    /// Sets the `.env` file of the `DotEnv` layer; `None` disables it. Unset, it is
    /// [`DEFAULT_DOTENV`] when the layer list has `DotEnv` (see [`layers`](Self::layers)
    /// and [`Bootstrapped::check_layers`]), and absent otherwise.
    #[must_use]
    pub fn dotenv_path(mut self, path: Option<PathBuf>) -> Self {
        self.dotenv_path = Some(path);
        self
    }

    /// Sets the `--set key=value` overrides.
    #[must_use]
    pub fn cli_overrides(mut self, overrides: CliOverrides) -> Self {
        self.cli_overrides = overrides;
        self
    }

    /// Reads `--config`, `--profile`, `--env-prefix`, and `--set key=value` (each also as
    /// `--flag=value`) from `args`, ignoring every other argument. For applications without
    /// clap; with clap use the `cli` helpers.
    ///
    /// `--set` is accepted only after [`layers`](Self::layers) named a list with
    /// `CliArgs`: without that layer the overrides would be dropped.
    ///
    /// # Errors
    ///
    /// `ConfigError::Parse` (origin `cli`) for a flag without a value, a malformed
    /// `--set`, or a `--set` the layer list has no `CliArgs` layer for.
    pub fn from_args<I, S>(mut self, args: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = args.into_iter().map(Into::into);
        while let Some(arg) = args.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) if flag.starts_with("--") => {
                    (flag.to_owned(), Some(value.to_owned()))
                }
                _ => (arg.clone(), None),
            };
            if !matches!(
                flag.as_str(),
                "--config" | "--profile" | "--env-prefix" | "--set"
            ) {
                continue;
            }
            let value = match inline {
                Some(value) => value,
                None => args.next().ok_or_else(|| {
                    ConfigError::parse(
                        cgp_config_core::Origin::cli(flag.as_str()),
                        None,
                        format!("{flag} needs a value"),
                    )
                })?,
            };
            match flag.as_str() {
                "--config" => self.config_path = Some(value.into()),
                "--profile" => self.profile = Some(value),
                "--env-prefix" => self.prefix = Some(value),
                _ if !self.accepts_cli => {
                    return Err(ConfigError::parse(
                        cgp_config_core::Origin::cli("--set"),
                        None,
                        "`--set` needs a loader with a `CliArgs` layer; name it with \
                         `Bootstrap::layers` (e.g. `TwelveFactorCliLayers`)",
                    ));
                }
                _ => self
                    .cli_overrides
                    .insert_assignment(&value)
                    .map_err(|message| {
                        ConfigError::parse(cgp_config_core::Origin::cli("--set"), None, message)
                    })?,
            }
        }
        Ok(self)
    }

    fn prefix(&self) -> String {
        self.prefix
            .clone()
            .unwrap_or_else(|| default_prefix(&self.app))
    }

    /// Resolves every setting. The file search touches the filesystem (existence checks
    /// only).
    #[must_use]
    pub fn finish(self) -> Bootstrapped {
        let exists = |path: &std::path::Path| path.is_file();
        self.finish_with(exists)
    }

    /// [`finish`](Self::finish) with a custom existence check (in-memory files).
    #[must_use]
    pub fn finish_with(self, exists: impl Fn(&std::path::Path) -> bool) -> Bootstrapped {
        let prefix = self.prefix();
        let mut env = self.env;
        let env_config = env.remove(&format!("{prefix}_CONFIG"));
        let env_profile = env.remove(&format!("{prefix}_PROFILE"));
        let env_document = env.remove(&format!("{prefix}_DOCUMENT"));
        let active_profile = self
            .profile
            .or(env_profile)
            .filter(|profile| !profile.is_empty())
            .unwrap_or_else(|| "default".to_owned());
        let config_location = match self.config_path.or_else(|| env_config.map(PathBuf::from)) {
            Some(path) => ConfigLocation::explicit(path),
            None => self
                .search
                .unwrap_or_else(|| SearchPath::standard(&self.app, &env))
                .resolve(exists)
                .require(self.require_file),
        };
        let dotenv_explicit = self.dotenv_path.is_some();
        let dotenv_path = match self.dotenv_path {
            Some(path) => path,
            None => self.uses_dotenv.then(|| PathBuf::from(DEFAULT_DOTENV)),
        };
        Bootstrapped {
            env_prefix: prefix,
            env_snapshot: env,
            active_profile,
            config_location,
            env_document,
            dotenv_path,
            dotenv_explicit,
            cli_overrides: self.cli_overrides,
        }
    }
}

/// Whether cgp-config reads the variable `key` under `prefix`.
fn is_read_variable(key: &str, prefix: &str) -> bool {
    if key == "CONFIGURATION_DIRECTORY" {
        return true;
    }
    let Some(rest) = key.strip_prefix(prefix) else {
        return false;
    };
    rest.starts_with(crate::env::SEPARATOR) || matches!(rest, "_CONFIG" | "_PROFILE" | "_DOCUMENT")
}

#[cfg(test)]
mod tests {
    // Some tests need a format to search for.
    #![cfg_attr(not(feature = "toml"), allow(unused_imports))]

    use std::path::PathBuf;

    use cgp_config_core::EnvSnapshot;

    use super::{Bootstrap, default_prefix};
    use crate::location::SearchPath;

    fn env() -> EnvSnapshot {
        EnvSnapshot::from_iter([
            ("APP_CONFIG", "/etc/app/app.toml"),
            ("APP_PROFILE", "staging"),
            ("APP_DOCUMENT", "{}"),
            ("APP__HTTP__PORT", "1"),
        ])
    }

    #[test]
    fn cli_beats_env_beats_builtin() {
        let done = Bootstrap::new("app").with_env(env()).finish_with(|_| false);
        assert_eq!(done.env_prefix, "APP");
        assert_eq!(done.active_profile, "staging");
        assert_eq!(
            done.config_location.file,
            Some(PathBuf::from("/etc/app/app.toml"))
        );
        assert!(done.config_location.required);
        assert_eq!(done.env_document.as_deref(), Some("{}"));
        let keys: Vec<_> = done.env_snapshot.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["APP__HTTP__PORT"], "bootstrap keys are removed");

        let done = Bootstrap::new("app")
            .with_env(env())
            .layers::<crate::layers::TwelveFactorCliLayers>()
            .from_args([
                "prog",
                "--profile",
                "prod",
                "--config=/x.toml",
                "--set",
                "a.b=1",
                "--other",
            ])
            .unwrap()
            .finish_with(|_| false);
        assert_eq!(done.active_profile, "prod");
        assert_eq!(done.config_location.file, Some(PathBuf::from("/x.toml")));
        assert_eq!(
            done.cli_overrides.iter().collect::<Vec<_>>(),
            [("a.b", "1")]
        );

        let done = Bootstrap::new("app")
            .search(SearchPath::new("app"))
            .finish_with(|_| false);
        assert_eq!(done.active_profile, "default");
        assert_eq!(done.config_location.file, None);
        assert!(!done.config_location.required);
        assert!(Bootstrap::new("app").from_args(["--profile"]).is_err());
        assert!(
            Bootstrap::new("app")
                .layers::<crate::layers::TwelveFactorCliLayers>()
                .from_args(["--set", "nokey"])
                .is_err()
        );
    }

    #[test]
    fn set_needs_a_cli_layer() {
        use crate::layers::{TwelveFactorCliLayers, TwelveFactorLayers};
        let error = Bootstrap::new("app")
            .from_args(["--set", "a=1"])
            .unwrap_err();
        assert!(error.to_string().contains("CliArgs"), "{error}");
        let error = Bootstrap::new("app")
            .layers::<TwelveFactorLayers>()
            .from_args(["--set=a=1"])
            .unwrap_err();
        assert_eq!(error.code(), "config::parse");
        // Other flags still work without the layer.
        let done = Bootstrap::new("app")
            .from_args(["--profile", "p"])
            .unwrap()
            .finish_with(|_| false);
        assert_eq!(done.active_profile, "p");
        let done = Bootstrap::new("app")
            .layers::<TwelveFactorCliLayers>()
            .from_args(["--set", "a=1"])
            .unwrap()
            .finish_with(|_| false);
        assert!(done.check_layers::<TwelveFactorCliLayers>().is_ok());
    }

    #[test]
    fn inputs_without_a_layer_are_refused() {
        use crate::layers::{
            TwelveFactorDocumentLayers, TwelveFactorDotenvLayers, TwelveFactorLayers,
        };
        let with_document = || {
            Bootstrap::new("app")
                .with_env(EnvSnapshot::from_iter([("APP_DOCUMENT", "{}")]))
                .finish_with(|_| false)
        };
        let error = with_document()
            .check_layers::<TwelveFactorLayers>()
            .unwrap_err();
        assert!(error.to_string().contains("APP_DOCUMENT"), "{error}");
        assert!(
            with_document()
                .check_layers::<TwelveFactorDocumentLayers>()
                .is_ok()
        );
        let explicit = Bootstrap::new("app")
            .dotenv_path(Some(PathBuf::from("/srv/.env")))
            .finish_with(|_| false);
        assert!(
            explicit
                .clone()
                .check_layers::<TwelveFactorLayers>()
                .is_err()
        );
        assert_eq!(
            explicit
                .check_layers::<TwelveFactorDotenvLayers>()
                .unwrap()
                .dotenv_path,
            Some(PathBuf::from("/srv/.env"))
        );
    }

    #[test]
    fn dotenv_defaults_with_a_dotenv_layer() {
        use crate::layers::{TwelveFactorDotenvLayers, TwelveFactorLayers};
        let plain = Bootstrap::new("app").finish_with(|_| false);
        assert_eq!(plain.dotenv_path, None);
        assert_eq!(
            plain
                .clone()
                .check_layers::<TwelveFactorDotenvLayers>()
                .unwrap()
                .dotenv_path,
            Some(PathBuf::from(".env"))
        );
        assert_eq!(
            plain
                .check_layers::<TwelveFactorLayers>()
                .unwrap()
                .dotenv_path,
            None
        );
        let named = Bootstrap::new("app")
            .layers::<TwelveFactorDotenvLayers>()
            .finish_with(|_| false);
        assert_eq!(named.dotenv_path, Some(PathBuf::from(".env")));
        let disabled = Bootstrap::new("app")
            .layers::<TwelveFactorDotenvLayers>()
            .dotenv_path(None)
            .finish_with(|_| false)
            .check_layers::<TwelveFactorDotenvLayers>()
            .unwrap();
        assert_eq!(disabled.dotenv_path, None);
    }

    #[test]
    #[cfg(all(unix, feature = "toml"))]
    fn search_uses_configuration_directory_from_the_snapshot() {
        let env = EnvSnapshot::from_iter([("CONFIGURATION_DIRECTORY", "/etc/svc:/other")]);
        let search = SearchPath::standard("svc", &env).with_working_dir(Some("/work".into()));
        let done = Bootstrap::new("svc")
            .with_env(env)
            .search(search)
            .require_file(true)
            .finish_with(|path: &std::path::Path| {
                path.starts_with("/etc/svc") || path.starts_with("/work")
            });
        assert_eq!(
            done.config_location.file,
            Some(PathBuf::from("/etc/svc/svc.toml"))
        );
        assert!(done.config_location.required);
    }

    #[test]
    fn prefixes_and_encoding() {
        assert_eq!(default_prefix("my-app.v2"), "MY_APP_V2");
        let custom = Bootstrap::new("app")
            .env_prefix("SVC")
            .with_env(EnvSnapshot::from_iter([("SVC_PROFILE", "p")]))
            .finish_with(|_| false);
        assert_eq!(custom.env_prefix, "SVC");
        assert_eq!(custom.active_profile, "p");

        #[cfg(unix)]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            let bad = OsString::from_vec(vec![0xff, 0xfe]);
            let error = Bootstrap::new("app")
                .capture_env_from([(OsString::from("APP__X"), bad.clone())])
                .unwrap_err();
            assert_eq!(error.code(), "config::env_encoding");
            let ok = Bootstrap::new("app")
                .capture_env_from([
                    (OsString::from("OTHER"), bad.clone()),
                    (OsString::from("APP__Y"), OsString::from("1")),
                ])
                .unwrap()
                .finish_with(|_| false);
            assert_eq!(ok.env_snapshot.get("APP__Y"), Some("1"));
            assert!(!ok.env_snapshot.contains("OTHER"));
            // Sharing the prefix's letters is not enough: `APPLE` and `APP_X` are skipped.
            for skipped in ["APPLE", "APP_X", "APPLICATION__X"] {
                let ok = Bootstrap::new("app")
                    .capture_env_from([(OsString::from(skipped), bad.clone())])
                    .unwrap()
                    .finish_with(|_| false);
                assert!(!ok.env_snapshot.contains(skipped));
            }
            for read in [
                "APP_CONFIG",
                "APP_PROFILE",
                "APP_DOCUMENT",
                "CONFIGURATION_DIRECTORY",
            ] {
                let error = Bootstrap::new("app")
                    .capture_env_from([(OsString::from(read), bad.clone())])
                    .unwrap_err();
                assert_eq!(error.code(), "config::env_encoding", "{read}");
            }
            let bad_key = OsString::from_vec(b"APP__\xff".to_vec());
            assert!(
                Bootstrap::new("app")
                    .capture_env_from([(bad_key, OsString::from("1"))])
                    .is_err()
            );
            let bad_other_key = OsString::from_vec(b"APPLE\xff".to_vec());
            assert!(
                Bootstrap::new("app")
                    .capture_env_from([(bad_other_key, OsString::from("1"))])
                    .is_ok()
            );
        }
    }
}
