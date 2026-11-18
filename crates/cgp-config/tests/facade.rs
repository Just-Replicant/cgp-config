//! The facade alone (plus `cgp-fork` and `serde`) is enough to build a loader.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]
#![cfg(feature = "std")]

use cgp_config::cgp::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_config::prelude::*;

#[derive(serde::Deserialize, Debug, PartialEq)]
struct Config {
    name: String,
    #[serde(default)]
    workers: u8,
    tags: Vec<String>,
}

struct Defaults;
struct Overrides;
struct Memory;

#[cgp_impl(Memory)]
impl SourceLoader<Defaults>
where
    Self: HasConfigValueType<ConfigValue = Value> + HasErrorType,
{
    fn load_source(&self, _: PhantomData<Defaults>) -> Result<Option<Value>, Self::Error> {
        let mut layer = Value::map([
            ("name", Value::from("svc")),
            ("tags", Value::array(["a".into()])),
            ("workres", Value::from(4)),
        ]);
        layer.stamp_origin(&Origin::Defaults);
        Ok(Some(layer))
    }
}

#[cgp_impl(Memory)]
impl SourceLoader<Overrides>
where
    Self: HasConfigValueType<ConfigValue = Value> + HasErrorType,
{
    fn load_source(&self, _: PhantomData<Overrides>) -> Result<Option<Value>, Self::Error> {
        let mut layer = Value::map([("tags", Value::from(r#"["b", "c", "d"]"#))]);
        layer.stamp_origin(&Origin::env("SVC__TAGS"));
        Ok(Some(layer))
    }
}

struct NonEmptyName;

#[cgp_impl(NonEmptyName)]
impl ConfigValidator<Config>
where
    Self: CanRaiseError<ConfigError>,
{
    fn validate_config(&self, config: &Config) -> Result<(), Self::Error> {
        let mut errors = ValidationErrors::new();
        if config.name.is_empty() {
            errors.push(ValidationIssue::new("name", "must not be empty"));
        }
        if config.tags.len() > 2 {
            errors.push(ValidationIssue::new("tags", "at most two tags"));
        }
        errors.into_result().map_err(Self::raise_error)
    }
}

#[derive(HasField)]
struct App {
    load_report: LoadReport,
}

delegate_components! {
    #[unchecked]
    App {
        preset CoreConfigPreset,
        ErrorTypeProviderComponent: cgp_fork::anyhow::UseAnyhowError,
        ErrorRaiserComponent: cgp_fork::anyhow::RaiseAnyhowError,
        SourceLoaderComponent: Memory,
        ConfigLoaderComponent: LoadLayered<Product![Defaults, Overrides]>,
        ConfigValidatorComponent: NonEmptyName,
    }
}

config_check! {
    App { config: Config, sources: [Defaults, Overrides] }
}

#[test]
fn layered_load_with_validation_and_warnings() {
    let app = App {
        load_report: LoadReport::new(),
    };
    let error = CanLoadConfig::<Config>::load_config(&app).unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "validation failed (1 error): tags: at most two tags"
    );
    assert_eq!(
        error.downcast_ref::<ConfigError>().unwrap().code(),
        "config::validation"
    );

    let warnings = app.load_report().take();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code(), "config::warn::unknown_key");
    assert_eq!(
        warnings[0].to_string(),
        "unknown key workres at defaults; did you mean \"workers\"?"
    );
}

#[test]
fn facade_reexports_cover_the_public_surface() {
    let value: cgp_config::Value = cgp_config::from_value(&Value::from(1)).unwrap();
    assert_eq!(value.as_u64(), Some(1));
    assert_eq!(cgp_config::REDACTED, "[REDACTED]");
    assert!(cgp_config::reserved::is_reserved("$version"));
    assert_eq!(cgp_config::LoadLimits::DEFAULT.max_depth, 64);
    assert_eq!(cgp_config::CoreConfigPreset::VERSION, 1);
    let _: Option<cgp_config::KeyPath> = cgp_config::KeyPath::parse("a.b[0]").ok();
}
