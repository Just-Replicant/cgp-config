//! Regression tests for the review fixes: bootstrap inputs reach (or are refused by) the
//! loader's layers, `--set` index overrides patch one array element, `{PREFIX}_DOCUMENT`
//! is canonicalized, profile tables are not re-read, `StrictPreset`, and the in-memory
//! shortcuts with a resolver.

#![cfg(feature = "toml")]
#![allow(
    clippy::std_instead_of_core,
    clippy::std_instead_of_alloc,
    missing_docs
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use cgp_config_core::components::*;
use cgp_config_core::{
    AggregateError, ConfigError, ConfigMeta, ConfigMetaField, ConfigSection, ConstDefault,
    FieldMeta, LoadReport, LoadWarning, Nested, NestedMeta, NestedShape, ValidateSections, nested,
};
use cgp_config_providers::bootstrap::Bootstrap;
use cgp_config_providers::layers::{TwelveFactorCliLayers, TwelveFactorLayers};
use cgp_config_providers::loader::{MemoryLoader, StdLoader, from_toml_str_with};
use cgp_config_providers::location::{ConfigLocation, SearchPath};
#[allow(clippy::wildcard_imports)]
use cgp_config_providers::presets::*;
use cgp_config_providers::read::{
    ConfigFileReader, ConfigFileReaderComponent, FileText, ReadStdFile,
};
use cgp_config_value::CoreConfigPreset;
use cgp_fork::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_fork::extra::error::RaiseFrom;
use cgp_fork::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
struct Server {
    host: String,
    port: u16,
}

impl ConfigSection for Server {
    const META: &'static ConfigMeta = &ConfigMeta::new(
        "Server",
        &[
            FieldMeta::new("host", "host").with_required(true),
            FieldMeta::new("port", "port").with_default(ConstDefault::UInt(80)),
        ],
    )
    .with_deny_unknown(true);
}

impl<Context: HasErrorType> ValidateSections<Context> for Server {
    fn validate_sections(&self, _context: &Context) -> Result<(), Context::Error> {
        Ok(())
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct App {
    name: String,
    #[serde(default)]
    servers: Vec<Server>,
}

impl ConfigSection for App {
    const META: &'static ConfigMeta = &ConfigMeta::new(
        "App",
        &[
            FieldMeta::new("name", "name")
                .with_required(true)
                .with_aliases(&["app_name"]),
            FieldMeta::new("servers", "servers")
                .with_nested(NestedMeta::new(NestedShape::Seq, Server::meta)),
        ],
    );
}

impl<Context> ValidateSections<Context> for App
where
    Context: CanValidateConfig<Server>,
    Context::Error: AggregateError,
{
    fn validate_sections(&self, context: &Context) -> Result<(), Context::Error> {
        nested::finish(nested::seq(context, "servers", &self.servers, None))
    }
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn boot(dir: &Path, env: &[(&str, &str)]) -> Bootstrap {
    Bootstrap::new("app")
        .with_env(env.iter().copied().collect())
        .search(SearchPath::new("app").with_working_dir(Some(dir.to_path_buf())))
}

const SERVERS: &str = "name = \"svc\"\n[[servers]]\nhost = \"a\"\nport = 1\n\
                       [[servers]]\nhost = \"b\"\nport = 2\n[[servers]]\nhost = \"c\"\nport = 3\n";

#[test]
fn set_without_a_cli_layer_is_refused_not_dropped() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "app.toml", SERVERS);
    // Bootstrap that does not know the layers refuses `--set` outright.
    assert!(
        boot(dir.path(), &[])
            .from_args(["--set", "name=x"])
            .is_err()
    );
    // Overrides set programmatically are refused by a loader without `CliArgs`.
    let done = boot(dir.path(), &[])
        .cli_overrides(cgp_config_providers::sources::CliOverrides::new().with("name", "x"))
        .finish();
    let error = StdLoader::for_section::<App>(done).unwrap_err();
    assert!(error.to_string().contains("CliArgs"), "{error}");
    // `{PREFIX}_DOCUMENT` without an `EnvDocument` layer is refused too.
    let done = boot(dir.path(), &[("APP_DOCUMENT", "{}")]).finish();
    let error = StdLoader::<TwelveFactorLayers>::with_layers(done).unwrap_err();
    assert!(error.to_string().contains("APP_DOCUMENT"), "{error}");
}

#[test]
fn set_with_an_index_patches_one_element() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "app.toml", SERVERS);
    let done = boot(dir.path(), &[])
        .layers::<TwelveFactorCliLayers>()
        .from_args(["--set", "servers[1].host=x", "--set", "servers[3].host=d"])
        .unwrap()
        .finish();
    let loader = StdLoader::<TwelveFactorCliLayers>::with_layers(done)
        .unwrap()
        .with_section::<App>();
    let config: App = loader.load().unwrap();
    let hosts: Vec<_> = config.servers.iter().map(|s| s.host.as_str()).collect();
    assert_eq!(
        hosts,
        ["a", "x", "c", "d"],
        "only element 1 changes, 3 appends"
    );
    assert_eq!(config.servers[1].port, 2, "the rest of element 1 stays");
    assert_eq!(
        config.servers[3].port, 80,
        "an appended element gets defaults"
    );

    let done = boot(dir.path(), &[])
        .layers::<TwelveFactorCliLayers>()
        .from_args(["--set", "servers[7].host=z"])
        .unwrap()
        .finish();
    let loader = StdLoader::<TwelveFactorCliLayers>::with_layers(done)
        .unwrap()
        .with_section::<App>();
    let error = loader.load::<App>().unwrap_err();
    assert!(error.to_string().contains("past the end"), "{error}");
}

#[cfg(feature = "json")]
#[test]
fn env_document_is_canonicalized() {
    use cgp_config_providers::layers::TwelveFactorDocumentLayers;

    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "app.toml", "name = \"file\"\n");
    let done = boot(dir.path(), &[("APP_DOCUMENT", r#"{"app_name": "doc"}"#)]).finish();
    let loader = StdLoader::<TwelveFactorDocumentLayers>::with_layers(done)
        .unwrap()
        .with_section::<App>();
    // The alias in the document renames to `name` before the merge, so it wins over the
    // file by precedence (instead of colliding with `name` as a duplicate).
    let config: App = loader.load().unwrap();
    assert_eq!(config.name, "doc");
    assert!(
        loader
            .load_report
            .warnings()
            .iter()
            .any(|w| matches!(w, LoadWarning::DeprecatedKey { .. })),
    );
}

/// A context that counts reads of the main file.
#[derive(HasField)]
struct Counting {
    env_snapshot: cgp_config_core::EnvSnapshot,
    env_prefix: String,
    active_profile: String,
    config_location: ConfigLocation,
    config_meta: Option<&'static ConfigMeta>,
    load_report: LoadReport,
    reads: AtomicUsize,
}

pub struct CountReads;

#[cgp_impl(CountReads: ConfigFileReaderComponent)]
impl ConfigFileReader
where
    Self: HasField<Symbol!("reads"), Value = AtomicUsize>
        + CanRaiseError<ConfigError>
        + cgp_config_core::HasLoadLimits
        + cgp_config_core::CanReportWarning,
{
    fn read_config_file(&self, path: &Path) -> Result<Option<FileText>, Self::Error> {
        if path.file_name().is_some_and(|name| name == "app.toml") {
            self.get_field(PhantomData::<Symbol!("reads")>)
                .fetch_add(1, Ordering::SeqCst);
        }
        <ReadStdFile as ConfigFileReader<Self>>::read_config_file(self, path)
    }

    fn list_config_dir(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>, Self::Error> {
        <ReadStdFile as ConfigFileReader<Self>>::list_config_dir(self, dir)
    }
}

delegate_components! {
    #[unchecked]
    Counting {
        preset TwelveFactorPreset,
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        RootConfigMetaComponent: ConfigMetaField,
        ConfigValidatorComponent: Nested,
        ConfigFileReaderComponent: CountReads,
    }
}

#[test]
fn the_main_file_is_read_once_per_load() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "app.toml",
        "name = \"base\"\n[\"$profiles\".staging]\nname = \"staging\"\n",
    );
    let done = boot(dir.path(), &[("APP_PROFILE", "staging")]).finish();
    let context = Counting {
        env_snapshot: done.env_snapshot,
        env_prefix: done.env_prefix,
        active_profile: done.active_profile,
        config_location: done.config_location,
        config_meta: Some(App::META),
        load_report: LoadReport::new(),
        reads: AtomicUsize::new(0),
    };
    let config: App = context.load_config().unwrap();
    assert_eq!(config.name, "staging");
    assert_eq!(context.reads.load(Ordering::SeqCst), 1);
    assert!(
        !context
            .load_report
            .warnings()
            .iter()
            .any(|w| matches!(w, LoadWarning::MissingProfile { .. })),
        "the profile table counts as a definition"
    );
}

#[derive(HasField)]
struct Strict {
    env_snapshot: cgp_config_core::EnvSnapshot,
    env_prefix: String,
    active_profile: String,
    config_location: ConfigLocation,
    config_meta: Option<&'static ConfigMeta>,
    load_report: LoadReport,
}

delegate_components! {
    #[unchecked]
    Strict {
        preset StrictPreset,
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        RootConfigMetaComponent: ConfigMetaField,
        ConfigValidatorComponent: Nested,
    }
}

fn strict(dir: &Path, profile: &str) -> Strict {
    let done = boot(dir, &[("APP_PROFILE", profile)]).finish();
    Strict {
        env_snapshot: done.env_snapshot,
        env_prefix: done.env_prefix,
        active_profile: done.active_profile,
        config_location: done.config_location,
        config_meta: Some(App::META),
        load_report: LoadReport::new(),
    }
}

#[test]
fn strict_preset_denies_unknown_keys_and_undefined_profiles() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "app.toml",
        "name = \"base\"\n[\"$profiles\".staging]\nname = \"staging\"\n",
    );
    let config: App = strict(dir.path(), "staging").load_config().unwrap();
    assert_eq!(config.name, "staging");
    let error = CanLoadConfig::<App>::load_config(&strict(dir.path(), "qa")).unwrap_err();
    assert_eq!(error.code(), "config::missing_profile", "{error}");
    // A profile file defines the profile too.
    write(dir.path(), "app.qa.toml", "name = \"qa\"\n");
    let config: App = strict(dir.path(), "qa").load_config().unwrap();
    assert_eq!(config.name, "qa");

    write(dir.path(), "app.toml", "name = \"base\"\nnmae = \"typo\"\n");
    let error = CanLoadConfig::<App>::load_config(&strict(dir.path(), "default")).unwrap_err();
    assert_eq!(error.code(), "config::unknown", "{error}");
}

#[test]
fn shortcuts_take_a_resolver_and_return_warnings() {
    let loaded = from_toml_str_with::<App>("app_name = \"svc\"\n", |config| {
        config.name.push_str("-resolved");
        Ok(())
    })
    .unwrap();
    assert_eq!(loaded.config.name, "svc-resolved");
    assert!(
        loaded
            .warnings
            .iter()
            .any(|w| matches!(w, LoadWarning::DeprecatedKey { .. })),
        "{:?}",
        loaded.warnings
    );
    let error = from_toml_str_with::<App>("name = \"svc\"\n", |_| {
        Err(ConfigError::type_error(
            "name",
            "denied by the test resolver",
            None,
        ))
    })
    .unwrap_err();
    assert!(
        error.to_string().contains("denied by the test resolver"),
        "{error}"
    );
}

#[test]
fn deny_unknown_keys_join_missing_field_errors() {
    // `servers[0]` misses `host` (required) and has `hots` (unknown, deny_unknown):
    // both are reported in one run.
    let error = MemoryLoader::from_toml::<App>("name = \"svc\"\n[[servers]]\nhots = \"a\"\n")
        .unwrap()
        .load::<App>()
        .unwrap_err();
    let ConfigError::Validation(errors) = &error else {
        panic!("expected aggregated errors, got {error}");
    };
    let text = error.to_string();
    assert!(errors.len() >= 2, "{text}");
    assert!(text.contains("servers[0].host"), "{text}");
    assert!(text.contains("hots"), "{text}");
}

#[test]
fn memory_loader_needs_no_file_fields() {
    // `TestConfigPreset` wires no file reader, so the context has no `config_location`
    // (or other file field) and the load still works.
    let loader = MemoryLoader::from_toml::<App>("name = \"m\"\n").unwrap();
    let config: App = loader.load().unwrap();
    assert_eq!(config.name, "m");
}
