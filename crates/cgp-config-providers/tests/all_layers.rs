//! A custom context with every standard layer (dotenv and CLI included), files served
//! from memory by `ReadFromMap`, and a validator table that aggregates a rule and the
//! nested sections.

#![cfg(all(feature = "toml", feature = "dotenv", feature = "clap"))]
#![allow(missing_docs)]

use std::path::{Path, PathBuf};

use cgp_config_core::components::*;
use cgp_config_core::{
    AggregateError, ConfigMeta, ConfigSection, EnvSnapshot, FieldMeta, NestedMeta, NestedShape,
    ValidateSections, nested,
};
use cgp_config_providers::bootstrap::Bootstrap;
use cgp_config_providers::location::SearchPath;
use cgp_fork::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Http {
    port: u16,
}

impl ConfigSection for Http {
    const META: &'static ConfigMeta = &ConfigMeta::new("Http", &[FieldMeta::new("port", "port")]);
}

impl<Context: HasErrorType> ValidateSections<Context> for Http {
    fn validate_sections(&self, _context: &Context) -> Result<(), Context::Error> {
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct App {
    name: String,
    http: Http,
}

impl ConfigSection for App {
    const META: &'static ConfigMeta = &ConfigMeta::new(
        "App",
        &[
            FieldMeta::new("name", "name"),
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

use cgp_config_core::{
    AllOf, ConfigError, ConfigMetaField, LoadReport, Nested, ValidateFn, ValidationErrors,
    ValidationIssue, ValidationRule,
};
use cgp_config_providers::bootstrap::Bootstrapped;
use cgp_config_providers::location::ConfigLocation;
use cgp_config_providers::presets::*;
use cgp_config_providers::read::{ConfigFileReaderComponent, ReadFromMap, VirtualFiles};
use cgp_config_providers::sources::CliOverrides;
use cgp_config_value::CoreConfigPreset;
use cgp_fork::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_fork::extra::error::RaiseFrom;

#[derive(HasField)]
struct FullLoader {
    env_snapshot: EnvSnapshot,
    env_prefix: String,
    active_profile: String,
    config_location: ConfigLocation,
    cli_overrides: CliOverrides,
    dotenv_path: Option<PathBuf>,
    virtual_files: VirtualFiles,
    config_meta: Option<&'static ConfigMeta>,
    load_report: LoadReport,
}

pub struct PortRule;

impl ValidationRule<App> for PortRule {
    fn check(config: &App, errors: &mut ValidationErrors) {
        if config.http.port < 1024 {
            errors.push(ValidationIssue::new("http.port", "must be at least 1024"));
        }
        if config.name.is_empty() {
            errors.push(ValidationIssue::new("name", "must not be empty"));
        }
    }
}

delegate_components! {
    #[unchecked]
    FullLoader {
        preset TwelveFactorDotenvCli,
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        RootConfigMetaComponent: ConfigMetaField,
        ConfigFileReaderComponent: ReadFromMap,
        ConfigValidatorComponent: UseDelegate<new FullValidators {
            App: AllOf<Product![ValidateFn<PortRule>, Nested]>,
            Http: Nested,
        }>,
    }
}

fn full(boot: Bootstrapped, files: VirtualFiles) -> FullLoader {
    FullLoader {
        env_snapshot: boot.env_snapshot,
        env_prefix: boot.env_prefix,
        active_profile: boot.active_profile,
        config_location: boot.config_location,
        cli_overrides: boot.cli_overrides,
        dotenv_path: Some(PathBuf::from("/srv/.env")),
        virtual_files: files,
        config_meta: Some(App::META),
        load_report: LoadReport::new(),
    }
}

#[test]
fn dotenv_env_and_cli_in_order() {
    let files = VirtualFiles::new()
        .with("/srv/app.toml", "name = \"file\"\n[http]\nport = 2000\n")
        .with(
            "/srv/.env",
            "APP__NAME=dotenv\nAPP__HTTP__PORT=3000\nUNRELATED=1\n",
        );
    let exists = |path: &Path| path == Path::new("/srv/app.toml");
    let boot = Bootstrap::new("app")
        .with_env(EnvSnapshot::from_iter([("APP__HTTP__PORT", "4000")]))
        .search(SearchPath::new("app").with_working_dir(Some("/srv".into())))
        .from_args(["--set", "http.port=5000"])
        .unwrap()
        .finish_with(exists);
    let loader = full(boot, files.clone());
    let config: App = loader.load_config().unwrap();
    assert_eq!(config.name, "dotenv");
    assert_eq!(config.http.port, 5000, "cli wins");

    // All validation problems arrive together.
    let boot = Bootstrap::new("app")
        .search(SearchPath::new("app").with_working_dir(Some("/srv".into())))
        .from_args(["--set", "http.port=80", "--set", "name="])
        .unwrap()
        .finish_with(exists);
    let error = CanLoadConfig::<App>::load_config(&full(boot, files)).unwrap_err();
    let ConfigError::Validation(issues) = &error else {
        panic!("expected validation, got {error}");
    };
    assert_eq!(issues.len(), 2, "{error}");
}
