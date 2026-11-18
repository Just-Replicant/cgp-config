//! `DefaultPreparers` in `CoreConfigPreset`: file markers run `PrepareFileLayer`
//! (`$version`, `$include`, `$profiles`), every other built-in marker passes through.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

use cgp_config_core::components::*;
use cgp_config_core::markers::{ConfigFile, EnvVars};
use cgp_config_core::{ConfigError, LoadLayered, LoadReport, LoadWarning, Origin, config_check};
use cgp_config_value::{ConfigValue, CoreConfigPreset};
use cgp_fork::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_fork::extra::error::RaiseFrom;
use cgp_fork::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
struct Http {
    port: u16,
    host: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct App {
    name: String,
    http: Http,
}

/// Serves the `file` and `env` fields as the `ConfigFile` and `EnvVars` layers.
struct Fields;

#[cgp_impl(Fields)]
impl SourceLoader<ConfigFile>
where
    Self: HasField<Symbol!("file"), Value = Option<ConfigValue>>
        + HasConfigValueType<ConfigValue = ConfigValue>
        + HasErrorType,
{
    fn load_source(&self, _: PhantomData<ConfigFile>) -> Result<Option<ConfigValue>, Self::Error> {
        Ok(self.get_field(PhantomData::<Symbol!("file")>).clone())
    }
}

#[cgp_impl(Fields)]
impl SourceLoader<EnvVars>
where
    Self: HasField<Symbol!("env"), Value = Option<ConfigValue>>
        + HasConfigValueType<ConfigValue = ConfigValue>
        + HasErrorType,
{
    fn load_source(&self, _: PhantomData<EnvVars>) -> Result<Option<ConfigValue>, Self::Error> {
        Ok(self.get_field(PhantomData::<Symbol!("env")>).clone())
    }
}

/// The file preparer needs `active_profile`; nothing else in the preset does.
#[derive(HasField)]
struct Loader {
    file: Option<ConfigValue>,
    env: Option<ConfigValue>,
    active_profile: String,
    load_report: LoadReport,
}

delegate_components! {
    #[unchecked]
    Loader {
        preset CoreConfigPreset,
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        SourceLoaderComponent: Fields,
        ConfigLoaderComponent: LoadLayered<Product![ConfigFile, EnvVars]>,
    }
}

config_check! {
    Loader {
        config: App,
        sources: [ConfigFile, EnvVars],
    }
}

fn file(value: ConfigValue) -> Option<ConfigValue> {
    let mut value = value;
    value.stamp_origin(&Origin::file("app.toml"));
    Some(value)
}

fn http(port: u16, host: &str) -> ConfigValue {
    ConfigValue::map([("port", port.into()), ("host", host.into())])
}

fn loader(file: Option<ConfigValue>, profile: &str) -> Loader {
    Loader {
        file,
        env: None,
        active_profile: profile.into(),
        load_report: LoadReport::new(),
    }
}

fn codes(loader: &Loader) -> Vec<&'static str> {
    loader
        .load_report()
        .warnings()
        .iter()
        .map(LoadWarning::code)
        .collect()
}

#[test]
fn version_is_recorded_and_stripped() {
    let loader = loader(
        file(ConfigValue::map([
            ("$version", 2.into()),
            ("name", "svc".into()),
            ("http", http(80, "h")),
        ])),
        "default",
    );
    let app: App = loader.load_config().unwrap();
    assert_eq!(app.name, "svc");
    // No unknown-key warning for `$version`: it was removed before deserializing.
    assert_eq!(
        loader.load_report().warnings(),
        [LoadWarning::DocumentVersion {
            origin: Origin::file("app.toml"),
            version: 2,
        }]
    );
}

#[test]
fn a_missing_version_is_assumed_and_include_is_ignored() {
    let loader = loader(
        file(ConfigValue::map([
            ("$include", ConfigValue::array(["other.toml".into()])),
            ("name", "svc".into()),
            ("http", http(80, "h")),
        ])),
        "default",
    );
    let _: App = loader.load_config().unwrap();
    assert_eq!(
        codes(&loader),
        [
            "config::warn::assumed_version",
            "config::warn::ignored_include"
        ]
    );
}

#[test]
fn profiles_merge_default_then_active_and_record_the_profile() {
    let document = || {
        file(ConfigValue::map([
            ("name", "svc".into()),
            ("http", http(8080, "0.0.0.0")),
            (
                "$profiles",
                ConfigValue::map([
                    (
                        "default",
                        ConfigValue::map([("http", ConfigValue::map([("host", "local".into())]))]),
                    ),
                    (
                        "prod",
                        ConfigValue::map([("http", ConfigValue::map([("port", 80.into())]))]),
                    ),
                ]),
            ),
        ]))
    };
    let prod: App = loader(document(), "prod").load_config().unwrap();
    assert_eq!(
        prod.http,
        Http {
            port: 80,
            host: "local".into()
        }
    );
    let default: App = loader(document(), "default").load_config().unwrap();
    assert_eq!(
        default.http,
        Http {
            port: 8080,
            host: "local".into()
        }
    );
    let other: App = loader(document(), "staging").load_config().unwrap();
    assert_eq!(other.http.port, 8080);

    // Provenance: the profile's nodes name the profile; the layer root stays the file's.
    let prod = loader(document(), "prod");
    let tree = prod
        .collect_layers(
            prod.empty_config_value(),
            PhantomData::<Product![ConfigFile]>,
        )
        .unwrap();
    let port = tree.get("http").and_then(|h| h.get("port")).unwrap();
    assert_eq!(port.meta().profile.as_deref(), Some("prod"));
    assert_eq!(port.meta().origin, Origin::file("app.toml"));
    assert_eq!(port.meta().shadowed.len(), 1);
    assert!(tree.get("name").unwrap().meta().profile.is_none());
    assert!(tree.meta().profile.is_none());
    assert!(tree.get("$profiles").is_none());
}

#[test]
fn malformed_reserved_keys_are_type_errors() {
    let bad_version = loader(
        file(ConfigValue::map([("$version", "two".into())])),
        "default",
    );
    let error = CanLoadConfig::<App>::load_config(&bad_version).unwrap_err();
    assert_eq!(error.code(), "config::type");
    assert_eq!(
        error.to_string(),
        "$version: expected a schema version (a non-negative integer), found string at app.toml"
    );

    let bad_profiles = loader(file(ConfigValue::map([("$profiles", 1.into())])), "default");
    let error = CanLoadConfig::<App>::load_config(&bad_profiles).unwrap_err();
    assert_eq!(
        error.path().map(ToString::to_string).as_deref(),
        Some("$profiles")
    );

    let bad_profile = loader(
        file(ConfigValue::map([(
            "$profiles",
            ConfigValue::map([("prod", 1.into())]),
        )])),
        "prod",
    );
    let error = CanLoadConfig::<App>::load_config(&bad_profile).unwrap_err();
    assert_eq!(
        error.path().map(ToString::to_string).as_deref(),
        Some("$profiles.prod")
    );
}

#[test]
fn non_file_markers_pass_through_unchanged() {
    // `EnvVars` uses `IdentityLayer`: a reserved-looking key in env is not interpreted.
    let mut env = ConfigValue::map([("$version", "3".into())]);
    env.stamp_origin(&Origin::env("APP__$VERSION"));
    let loader = Loader {
        env: Some(env.clone()),
        ..loader(
            file(ConfigValue::map([
                ("name", "svc".into()),
                ("http", http(80, "h")),
            ])),
            "default",
        )
    };
    assert!(ConfigValue::ptr_eq(
        &loader
            .prepare_layer(PhantomData::<EnvVars>, env.clone())
            .unwrap(),
        &env
    ));
    let _: App = loader.load_config().unwrap();
    assert_eq!(
        codes(&loader),
        ["config::warn::assumed_version", "config::warn::unknown_key"]
    );
}
