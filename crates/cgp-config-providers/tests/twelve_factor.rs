//! End-to-end loads through the standard presets, with hand-written section metadata
//! (the derive is tested in the facade). Hermetic: env comes from `EnvSnapshot`, files
//! from a fresh temporary directory or `ReadFromMap`; nothing changes the process
//! environment or the working directory.

#![cfg(feature = "toml")]
#![allow(
    clippy::std_instead_of_core,
    clippy::std_instead_of_alloc,
    missing_docs
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use cgp_config_core::components::*;
use cgp_config_core::{
    AggregateError, ConfigMeta, ConfigSection, ConstDefault, EnvSnapshot, FieldMeta, LoadWarning,
    NestedMeta, NestedShape, ParseKind, ValidateSections, nested,
};
use cgp_config_providers::bootstrap::Bootstrap;
use cgp_config_providers::loader::{LoadSection, MemoryLoader, StdLoader};
use cgp_config_providers::location::SearchPath;
use cgp_fork::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
struct Http {
    port: u16,
    timeout: Duration,
}

impl ConfigSection for Http {
    const META: &'static ConfigMeta = &ConfigMeta::new(
        "Http",
        &[
            FieldMeta::new("port", "port")
                .with_default(ConstDefault::UInt(8080))
                .with_env_raw("PORT"),
            FieldMeta::new("timeout", "timeout")
                .with_default(ConstDefault::Str("30s"))
                .with_parse(ParseKind::Duration),
        ],
    );
}

impl<Context: HasErrorType> ValidateSections<Context> for Http {
    fn validate_sections(&self, _context: &Context) -> Result<(), Context::Error> {
        Ok(())
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct App {
    name: String,
    http: Http,
}

impl ConfigSection for App {
    const META: &'static ConfigMeta = &ConfigMeta::new(
        "App",
        &[
            FieldMeta::new("name", "name").with_required(true),
            FieldMeta::new("http", "http")
                .with_nested(NestedMeta::new(NestedShape::Single, Http::meta)),
        ],
    );
}

impl<Context> ValidateSections<Context> for App
where
    Context: CanValidateConfig<Http>,
    Context::Error: AggregateError,
{
    fn validate_sections(&self, context: &Context) -> Result<(), Context::Error> {
        nested::finish(nested::single(context, "http", &self.http, None))
    }
}

fn boot(dir: &Path, env: &[(&str, &str)]) -> Bootstrap {
    Bootstrap::new("app")
        .with_env(env.iter().copied().collect())
        .search(SearchPath::new("app").with_working_dir(Some(dir.to_path_buf())))
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn layers_apply_in_order() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "app.toml",
        "name = \"base\"\n[http]\nport = 1000\n",
    );
    write(
        dir.path(),
        "app.prod.toml",
        "name = \"prod\"\n[http]\ntimeout = \"1m\"\n",
    );

    let loader = StdLoader::for_section::<App>(boot(dir.path(), &[]).finish());
    let config: App = loader.load_config().unwrap();
    assert_eq!(
        config,
        App {
            name: "base".into(),
            http: Http {
                port: 1000,
                timeout: Duration::from_secs(30)
            }
        }
    );

    let env = [("APP_PROFILE", "prod"), ("APP__HTTP__PORT", "2000")];
    let loader = StdLoader::for_section::<App>(boot(dir.path(), &env).finish());
    let config: App = loader.load_config().unwrap();
    assert_eq!(config.name, "prod");
    assert_eq!(config.http.port, 2000);
    assert_eq!(config.http.timeout, Duration::from_secs(60));
    // Files without `$version` are assumed to be version 1 (migrations arrive in 0.2);
    // nothing else is reported.
    let warnings = loader.load_report.warnings();
    assert!(
        warnings
            .iter()
            .all(|w| matches!(w, LoadWarning::AssumedVersion { .. })),
        "{warnings:?}"
    );
}

#[test]
fn no_file_runs_on_env_and_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let env = [("APP__NAME", "envonly"), ("PORT", "7000")];
    let loader = StdLoader::for_section::<App>(boot(dir.path(), &env).finish());
    let config: App = loader.load_config().unwrap();
    assert_eq!(config.name, "envonly");
    assert_eq!(config.http.port, 7000, "env_raw PORT");

    let loader = StdLoader::for_section::<App>(boot(dir.path(), &[]).finish());
    let error = loader.load::<App>().unwrap_err();
    assert!(error.to_string().contains("name"), "{error}");

    let loader = StdLoader::for_section::<App>(boot(dir.path(), &[]).require_file(true).finish());
    let error = loader.load::<App>().unwrap_err();
    assert_eq!(error.code(), "config::missing");
    assert!(
        error.to_string().contains("app.toml"),
        "lists the searched paths: {error}"
    );
}

#[test]
fn explicit_file_is_required_and_errors_carry_positions() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.toml");
    let env = [("APP_CONFIG", missing.to_str().unwrap())];
    let error = StdLoader::for_section::<App>(boot(dir.path(), &env).finish())
        .load::<App>()
        .unwrap_err();
    assert_eq!(error.code(), "config::missing");

    let bad = write(dir.path(), "bad.toml", "name = \"x\"\n[http]\nport = = 1\n");
    let error = StdLoader::for_section::<App>(boot(dir.path(), &[]).config_path(&bad).finish())
        .load::<App>()
        .unwrap_err();
    let text = error.to_string();
    assert!(text.contains("bad.toml:3:"), "file:line:col in {text}");

    let bad = write(
        dir.path(),
        "bad.json",
        "{\"name\": \"x\",\n \"name\": \"y\"}",
    );
    let error = StdLoader::for_section::<App>(boot(dir.path(), &[]).config_path(&bad).finish())
        .load::<App>();
    #[cfg(feature = "json")]
    assert_eq!(error.unwrap_err().code(), "config::duplicate_key");
    #[cfg(not(feature = "json"))]
    assert_eq!(error.unwrap_err().code(), "config::parse");
}

#[test]
fn profiles_table_and_missing_profile_warning() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "app.toml",
        "name = \"base\"\n[\"$profiles\".staging]\nname = \"staging\"\n",
    );
    let loader =
        StdLoader::for_section::<App>(boot(dir.path(), &[("APP_PROFILE", "staging")]).finish());
    assert_eq!(loader.load::<App>().unwrap().name, "staging");

    let loader = StdLoader::for_section::<App>(boot(dir.path(), &[("APP_PROFILE", "qa")]).finish());
    assert_eq!(loader.load::<App>().unwrap().name, "base");
    let warnings = loader.load_report.warnings();
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, LoadWarning::MissingProfile { profile } if &**profile == "qa")),
        "{warnings:?}"
    );
}

#[test]
fn duration_needs_a_unit() {
    let error = App::from_toml_str("name = \"x\"\n[http]\ntimeout = 30\n").unwrap_err();
    assert!(error.to_string().contains("unit"), "{error}");
    let error = App::from_toml_str("name = \"x\"\n[http]\ntimeout = \"30\"\n").unwrap_err();
    assert!(error.to_string().contains("unit"), "{error}");
    let config = App::from_toml_str("name = \"x\"\n[http]\ntimeout = \"30s\"\n").unwrap();
    assert_eq!(config.http.timeout, Duration::from_secs(30));
}

#[test]
fn memory_loader_shortcuts() {
    let error = App::from_defaults().unwrap_err();
    assert_eq!(error.code(), "config::missing");
    let config = App::from_toml_str("name = \"m\"\n").unwrap();
    assert_eq!(config.http.port, 8080);
    let loader = MemoryLoader::from_toml::<App>("name = \"m\"\nextra = 1\n")
        .unwrap()
        .with_env("SVC", EnvSnapshot::from_iter([("SVC__HTTP__PORT", "9")]));
    let config: App = loader.load().unwrap();
    assert_eq!(config.http.port, 9);
    let warnings = loader.load_report.warnings();
    assert!(
        warnings.iter().any(|w| w.to_string().contains("extra")),
        "{warnings:?}"
    );
}
