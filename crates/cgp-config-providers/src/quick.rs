//! [`load`] and [`load_from`]: a twelve-factor load without a hand-written context.
//!
//! [`load`] reads the process environment once and searches the real filesystem
//! ([`SearchPath::standard`](crate::location::SearchPath::standard)). Libraries do not call
//! it. Tests and doctests call [`load_from`], which takes an [`EnvSnapshot`] and an
//! in-memory file map and never touches the process.

use std::path::{Path, PathBuf};

use cgp_config_core::{
    CanLoadConfig, ConfigError, ConfigLoaded, ConfigMeta, ConfigMetaField, ConfigSection,
    EnvSnapshot, LoadReport, Nested,
};
use cgp_fork::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_fork::extra::error::RaiseFrom;
use cgp_fork::prelude::*;

use crate::bootstrap::Bootstrap;
use crate::loader::{StdLoader, log_loaded};
use crate::location::SearchPath;
use crate::presets::__entries::*;
use crate::presets::TwelveFactorPreset;
use crate::read::{ReadFromMap, VirtualFiles};
use crate::sources::CliOverrides;

/// The directory [`load_from`] treats as the working directory. Relative file keys are
/// stored here, and the search looks here before anywhere else except
/// `CONFIGURATION_DIRECTORY`.
const VIRTUAL_ROOT: &str = "/cgp-config-quick";

/// Loads `Config` the twelve-factor way for `app`.
///
/// Reads the process environment once ([`Bootstrap::capture_env`](crate::bootstrap::Bootstrap::capture_env)),
/// then searches for `{app}.toml` in the order of
/// [`SearchPath::standard`](crate::location::SearchPath::standard): an explicit
/// `{PREFIX}_CONFIG`, `$CONFIGURATION_DIRECTORY`, the working directory
/// (`{app}.toml`, `config.toml`, `config/{app}.toml`), the executable's directory, and
/// the platform config directory. The prefix is the app name uppercased (`myapp` →
/// `MYAPP`), so a nested key is `MYAPP__HTTP__PORT`. The profile defaults to `default`.
///
/// Returns the config and the warnings of the load. A search that finds nothing leaves
/// the file layer absent; the load can still succeed from defaults and env.
///
/// This reads the process environment and the filesystem. Tests use [`load_from`].
///
/// # Errors
///
/// Any [`ConfigError`] of bootstrap or of the pipeline.
pub fn load<Config>(app: &str) -> Result<(Config, LoadReport), ConfigError>
where
    Config: ConfigSection,
    StdLoader: CanLoadConfig<Config, Error = ConfigError> + CanLog<ConfigLoaded>,
{
    let loader = StdLoader::for_section::<Config>(Bootstrap::new(app).capture_env()?.finish())?;
    let config = loader.load()?;
    Ok((config, loader.load_report.clone()))
}

/// [`load`] against `snapshot` and `files`, with no process IO.
///
/// Relative paths in `files` are placed in a fixed virtual working directory, and the
/// search order of [`load`] runs against that map. Absolute paths, and
/// `CONFIGURATION_DIRECTORY` / `{PREFIX}_CONFIG` when those variables are in `snapshot`,
/// are used as given. `{PREFIX}_PROFILE` selects the profile (default `default`).
/// Bootstrap keys are removed from the snapshot before the env layer runs.
///
/// # Errors
///
/// Any [`ConfigError`] of the pipeline.
pub fn load_from<Config, I, P, S>(
    app: &str,
    snapshot: EnvSnapshot,
    files: I,
) -> Result<(Config, LoadReport), ConfigError>
where
    Config: ConfigSection,
    I: IntoIterator<Item = (P, S)>,
    P: Into<PathBuf>,
    S: Into<String>,
    MapLoader: CanLoadConfig<Config, Error = ConfigError> + CanLog<ConfigLoaded>,
{
    let mut virtual_files = VirtualFiles::new();
    for (path, text) in files {
        virtual_files.insert(place(path.into()), text.into());
    }
    let search = SearchPath::new(app)
        .with_working_dir(Some(PathBuf::from(VIRTUAL_ROOT)))
        .with_configuration_directory(snapshot.get("CONFIGURATION_DIRECTORY").map(PathBuf::from))
        .with_exe_dir(None)
        .with_project_dir(None);
    let mut boot = Bootstrap::new(app)
        .with_env(snapshot)
        .search(search)
        .finish_with(|path| virtual_files.contains(path));
    if let Some(file) = boot.config_location.file.as_mut() {
        *file = place(std::mem::take(file));
    }
    let loader = MapLoader {
        env_snapshot: boot.env_snapshot,
        env_prefix: boot.env_prefix,
        active_profile: boot.active_profile,
        config_location: boot.config_location,
        env_document: boot.env_document,
        dotenv_path: boot.dotenv_path,
        cli_overrides: boot.cli_overrides,
        config_meta: Some(Config::META),
        load_report: LoadReport::new(),
        virtual_files,
    };
    let config = loader.load_config()?;
    log_loaded(&loader);
    Ok((config, loader.load_report.clone()))
}

/// Joins a relative path onto [`VIRTUAL_ROOT`]. Absolute paths and paths already under
/// the root stay as they are, so a search hit is not prefixed twice.
fn place(path: PathBuf) -> PathBuf {
    let root = Path::new(VIRTUAL_ROOT);
    if path.starts_with(root) || path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

/// [`StdLoader`] with [`ReadFromMap`] in place of the filesystem reader.
///
/// Public so [`load_from`]'s bound can name it. Constructing it is crate-private.
#[doc(hidden)]
#[derive(HasField)]
pub struct MapLoader {
    env_snapshot: EnvSnapshot,
    env_prefix: String,
    active_profile: String,
    config_location: crate::location::ConfigLocation,
    env_document: Option<String>,
    dotenv_path: Option<PathBuf>,
    cli_overrides: CliOverrides,
    config_meta: Option<&'static ConfigMeta>,
    load_report: LoadReport,
    virtual_files: VirtualFiles,
}

delegate_components! {
    #[unchecked]
    MapLoader {
        preset TwelveFactorPreset,
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        RootConfigMetaComponent: ConfigMetaField,
        ConfigValidatorComponent: Nested,
        ConfigFileReaderComponent: ReadFromMap,
    }
}

#[cfg(test)]
mod tests {
    use cgp_config_core::{
        ConfigMeta, ConfigSection, ConstDefault, EnvSnapshot, FieldMeta, LoadWarning,
        ValidateSections,
    };
    use cgp_fork::prelude::HasErrorType;

    use super::{load_from, place};
    use crate::loader::StdLoader;

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct App {
        #[serde(default = "port_default")]
        port: u16,
        #[serde(default = "name_default")]
        name: String,
    }

    fn port_default() -> u16 {
        8080
    }

    fn name_default() -> String {
        "demo".to_owned()
    }

    impl ConfigSection for App {
        const META: &'static ConfigMeta = &ConfigMeta::new(
            "App",
            &[
                FieldMeta::new("port", "port")
                    .with_default(ConstDefault::UInt(8080))
                    .with_type("u16"),
                FieldMeta::new("name", "name")
                    .with_default(ConstDefault::Str("demo"))
                    .with_type("String"),
            ],
        );
    }

    impl<Context: HasErrorType> ValidateSections<Context> for App {
        fn validate_sections(&self, _context: &Context) -> Result<(), Context::Error> {
            Ok(())
        }
    }

    #[test]
    #[cfg(feature = "toml")]
    fn load_from_merges_file_and_env() {
        let (config, report) = load_from::<App, _, _, _>(
            "myapp",
            EnvSnapshot::from_iter([("MYAPP__PORT", "7000")]),
            [("myapp.toml", "\"$version\" = 1\nname = \"from-file\"\n")],
        )
        .expect("load");
        assert_eq!(config.port, 7000);
        assert_eq!(config.name, "from-file");
        assert!(
            report
                .warnings()
                .iter()
                .all(|warning| matches!(warning, LoadWarning::DocumentVersion { version: 1, .. })),
            "{report:?}"
        );
    }

    #[test]
    #[cfg(feature = "toml")]
    fn profile_file_sits_beside_the_main_file() {
        let (config, _) = load_from::<App, _, _, _>(
            "myapp",
            EnvSnapshot::from_iter([("MYAPP_PROFILE", "prod")]),
            [
                ("myapp.toml", "name = \"base\"\nport = 1\n"),
                ("myapp.prod.toml", "port = 2\n"),
            ],
        )
        .expect("load");
        assert_eq!(config.name, "base");
        assert_eq!(config.port, 2);
    }

    #[test]
    #[cfg(feature = "toml")]
    fn explicit_config_path_in_the_snapshot_is_required() {
        let error = load_from::<App, _, _, _>(
            "myapp",
            EnvSnapshot::from_iter([("MYAPP_CONFIG", "missing.toml")]),
            std::iter::empty::<(&str, &str)>(),
        )
        .expect_err("missing explicit file");
        assert_eq!(error.code(), "config::missing");
    }

    #[test]
    fn relative_paths_land_in_the_virtual_root() {
        let placed = place("myapp.toml".into());
        assert!(placed.ends_with("myapp.toml"));
        assert_eq!(place(placed.clone()), placed);
    }

    #[test]
    fn builder_rejects_a_prefix_with_a_trailing_separator() {
        let error = StdLoader::builder()
            .app_name("demo")
            .env_prefix("APP_")
            .build::<App>();
        assert!(error.is_err(), "{error:?}");
    }
}
