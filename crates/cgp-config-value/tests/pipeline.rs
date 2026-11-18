//! End-to-end: a loader context wired with `CoreConfigPreset`, two in-memory sources, and
//! `LoadLayered`, checked with `config_check!`.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

use std::sync::atomic::{AtomicUsize, Ordering};

// Every component name, so the keys `CoreConfigPreset` replays resolve here (the facade
// prelude does the same for applications).
use cgp_config_core::components::*;
use cgp_config_core::{
    CollectInOrder, ConfigError, IdentityLayer, IgnoreWarnings, LoadLayered, LoadLimits,
    LoadLimitsField, LoadReport, LoadWarning, Meta, NoValidation, Origin, config_check,
};
use cgp_config_value::{
    ConfigValue, CoreConfigPreset, DenyUnknownKeys, SerdeWithProvenance, ShallowReplace,
};
use cgp_fork::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_fork::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
struct Http {
    port: u16,
    host: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct AppConfig {
    name: String,
    http: Http,
}

/// Source markers.
struct Base;
struct Env;

/// Serves `base` and `env` fields of the context as layers.
struct FieldSource;

#[cgp_impl(FieldSource)]
impl SourceLoader<Base>
where
    Self: HasField<Symbol!("base"), Value = Option<ConfigValue>>
        + HasConfigValueType<ConfigValue = ConfigValue>
        + HasErrorType,
{
    fn load_source(&self, _source: PhantomData<Base>) -> Result<Option<ConfigValue>, Self::Error> {
        Ok(self.get_field(PhantomData::<Symbol!("base")>).clone())
    }
}

#[cgp_impl(FieldSource)]
impl SourceLoader<Env>
where
    Self: HasField<Symbol!("env"), Value = Option<ConfigValue>>
        + HasConfigValueType<ConfigValue = ConfigValue>
        + HasErrorType,
{
    fn load_source(&self, _source: PhantomData<Env>) -> Result<Option<ConfigValue>, Self::Error> {
        Ok(self.get_field(PhantomData::<Symbol!("env")>).clone())
    }
}

static VALIDATIONS: AtomicUsize = AtomicUsize::new(0);

struct ValidateApp;

#[cgp_impl(ValidateApp)]
impl ConfigValidator<AppConfig>
where
    Self: CanRaiseError<ConfigError>,
{
    fn validate_config(&self, config: &AppConfig) -> Result<(), Self::Error> {
        VALIDATIONS.fetch_add(1, Ordering::SeqCst);
        if config.http.port < 1024 {
            return Err(Self::raise_error(ConfigError::validation(
                "http.port",
                "must be at least 1024",
            )));
        }
        Ok(())
    }
}

#[derive(HasField)]
struct Loader {
    base: Option<ConfigValue>,
    env: Option<ConfigValue>,
    load_report: LoadReport,
}

delegate_components! {
    #[unchecked]
    Loader {
        preset CoreConfigPreset,
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        SourceLoaderComponent: FieldSource,
        ConfigLoaderComponent: LoadLayered<Product![Base, Env]>,
        // Custom markers are not in the preset's `DefaultPreparers`, so the context lists
        // their preparers (leaving them out is a compile error).
        LayerPreparerComponent: UseDelegate<new LoaderPreparers {
            [Base, Env]: IdentityLayer,
        }>,
        ConfigValidatorComponent: UseDelegate<new LoaderValidators {
            AppConfig: ValidateApp,
            Http: NoValidation,
        }>,
    }
}

config_check! {
    Loader {
        config: AppConfig,
        sources: [Base, Env],
    }
}

use cgp_fork::extra::error::RaiseFrom;

fn file_layer() -> ConfigValue {
    let mut layer = ConfigValue::map([
        ("name", ConfigValue::from("demo")),
        (
            "http",
            ConfigValue::map([
                ("port", 8080.into()),
                ("host", "0.0.0.0".into()),
                ("prot", 1.into()),
            ]),
        ),
    ]);
    layer.stamp_origin(&Origin::file("config/app.toml"));
    layer
}

fn env_layer(port: &str) -> ConfigValue {
    let mut layer = ConfigValue::map([("http", ConfigValue::map([("port", port.into())]))]);
    layer.stamp_origin(&Origin::env("APP__HTTP__PORT"));
    layer
}

fn loader(env: Option<ConfigValue>) -> Loader {
    Loader {
        base: Some(file_layer()),
        env,
        load_report: LoadReport::new(),
    }
}

#[test]
fn later_layers_win_and_env_strings_are_coerced() {
    let loader = loader(Some(env_layer("9090")));
    let config: AppConfig = loader.load_config().unwrap();
    assert_eq!(
        config,
        AppConfig {
            name: "demo".into(),
            http: Http {
                port: 9090,
                host: "0.0.0.0".into()
            },
        }
    );
    assert!(VALIDATIONS.load(Ordering::SeqCst) >= 1);
}

#[test]
fn unknown_keys_warn_with_a_suggestion() {
    let loader = loader(None);
    let _: AppConfig = loader.load_config().unwrap();
    let warnings = loader.load_report().take();
    assert_eq!(warnings.len(), 1);
    match &warnings[0] {
        LoadWarning::UnknownKey {
            path,
            suggestion,
            meta,
        } => {
            assert_eq!(path.to_string(), "http.prot");
            assert_eq!(suggestion.as_deref(), Some("port"));
            assert_eq!(
                meta.as_ref().map(|m| m.origin.to_string()).as_deref(),
                Some("config/app.toml")
            );
        }
        other => panic!("unexpected warning {other:?}"),
    }
}

#[test]
fn absent_optional_sources_are_skipped() {
    let config: AppConfig = loader(None).load_config().unwrap();
    assert_eq!(config.http.port, 8080);
}

#[test]
fn type_errors_name_the_path_origin_and_override_chain() {
    let error =
        CanLoadConfig::<AppConfig>::load_config(&loader(Some(env_layer("http")))).unwrap_err();
    assert_eq!(error.code(), "config::type");
    assert_eq!(
        error.path().map(ToString::to_string).as_deref(),
        Some("http.port")
    );
    assert_eq!(
        error.to_string(),
        "http.port: expected u16, found string \"http\" at env APP__HTTP__PORT (overrides config/app.toml)"
    );
}

#[test]
fn validation_runs_after_deserialize() {
    let error =
        CanLoadConfig::<AppConfig>::load_config(&loader(Some(env_layer("80")))).unwrap_err();
    assert_eq!(
        error.to_string(),
        "validation failed (1 error): http.port: must be at least 1024"
    );
}

#[test]
fn merge_conflicts_fail_the_load() {
    let mut env = ConfigValue::map([("http", ConfigValue::from("oops"))]);
    env.stamp_origin(&Origin::env("APP__HTTP"));
    let error = CanLoadConfig::<AppConfig>::load_config(&loader(Some(env))).unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot merge http: map from config/app.toml vs string from env APP__HTTP"
    );
}

#[test]
fn preset_components_are_usable_directly() {
    let loader = loader(None);
    assert!(
        loader
            .empty_config_value()
            .as_map()
            .is_some_and(|m| m.is_empty())
    );
    assert_eq!(loader.load_limits(), LoadLimits::DEFAULT);
    let merged = loader
        .merge_layers(
            ConfigValue::map([("a", 1.into())]),
            ConfigValue::map([("b", 2.into())]),
        )
        .unwrap();
    assert_eq!(
        merged.as_map().map(cgp_config_value::ConfigMap::len),
        Some(2)
    );
    loader.report_warning(LoadWarning::MissingProfile {
        profile: "prod".into(),
    });
    assert_eq!(loader.load_report().len(), 1);
    assert_eq!(CoreConfigPreset::VERSION, 1);
}

// A second context: anyhow as the error type, strict unknown keys, own limits, shallow
// merge, and warnings dropped. Explicit entries replace preset entries.

#[derive(HasField)]
struct StrictLoader {
    base: Option<ConfigValue>,
    env: Option<ConfigValue>,
    load_limits: LoadLimits,
}

delegate_components! {
    #[unchecked]
    StrictLoader {
        preset CoreConfigPreset,
        ErrorTypeProviderComponent: cgp_fork::anyhow::UseAnyhowError,
        ErrorRaiserComponent: cgp_fork::anyhow::RaiseAnyhowError,
        SourceLoaderComponent: FieldSource,
        ConfigLoaderComponent: LoadLayered<Product![Base, Env]>,
        ConfigDeserializerComponent: SerdeWithProvenance<DenyUnknownKeys>,
        LayerPreparerComponent: UseDelegate<LoaderPreparers>,
        LayerMergerComponent: ShallowReplace,
        LoadLimitsComponent: LoadLimitsField,
        LoadReportComponent: IgnoreWarnings,
    }
}

config_check! {
    #[check_trait(CanUseStrictLoader)]
    StrictLoader {
        config: AppConfig,
        sources: [Base, Env],
    }
}

#[test]
fn deny_unknown_keys_with_anyhow() {
    let strict = StrictLoader {
        base: Some(file_layer()),
        env: None,
        load_limits: LoadLimits::DEFAULT,
    };
    let error = CanLoadConfig::<AppConfig>::load_config(&strict).unwrap_err();
    let config_error = error.downcast_ref::<ConfigError>().unwrap();
    assert_eq!(config_error.code(), "config::unknown");
    assert_eq!(
        error.to_string(),
        "unknown key http.prot at config/app.toml; did you mean \"port\"?"
    );
    strict.report_warning(LoadWarning::MissingProfile {
        profile: "x".into(),
    });
}

#[test]
fn context_limits_apply_to_merges() {
    let strict = StrictLoader {
        base: Some(file_layer()),
        env: Some(env_layer("1")),
        load_limits: LoadLimits::DEFAULT.with_max_keys(2),
    };
    let error = CanLoadConfig::<AppConfig>::load_config(&strict).unwrap_err();
    assert_eq!(error.to_string(), "limit exceeded: max_keys (2)");
}

#[test]
fn shallow_replace_drops_lower_siblings() {
    let strict = StrictLoader {
        base: Some(file_layer()),
        env: Some(env_layer("9090")),
        load_limits: LoadLimits::DEFAULT,
    };
    // `http` from env replaces `http` from the file whole, so `host` is missing.
    let error = CanLoadConfig::<AppConfig>::load_config(&strict).unwrap_err();
    assert_eq!(error.to_string(), "missing http.host");
    let meta = Meta::new(Origin::Defaults);
    assert!(meta.shadowed.is_empty());
}

#[test]
fn collect_in_order_is_usable_on_its_own() {
    // `CollectInOrder` with an empty list returns the accumulator unchanged.
    use cgp_config_core::CanCollectLayers;
    let loader = loader(None);
    let acc = ConfigValue::map([("k", 1.into())]);
    let out = loader
        .collect_layers(acc.clone(), PhantomData::<Product![]>)
        .unwrap();
    assert!(ConfigValue::ptr_eq(&acc, &out));
    let _ = CollectInOrder;
}

#[test]
fn the_collected_root_names_its_layers() {
    use cgp_config_core::CanCollectLayers;
    // Only the file: the root is the file's root (same allocation), not "unknown".
    let only_file = loader(None);
    let file = only_file.base.clone().unwrap();
    let tree = only_file
        .collect_layers(
            only_file.empty_config_value(),
            PhantomData::<Product![Base, Env]>,
        )
        .unwrap();
    assert!(ConfigValue::ptr_eq(&tree, &file));
    assert_eq!(tree.meta().origin, Origin::file("config/app.toml"));

    // File then env: the root names env and records the file.
    let both = loader(Some(env_layer("9090")));
    let tree = both
        .collect_layers(
            both.empty_config_value(),
            PhantomData::<Product![Base, Env]>,
        )
        .unwrap();
    assert_eq!(tree.meta().origin, Origin::env("APP__HTTP__PORT"));
    assert_eq!(
        tree.meta().shadowed.first().map(|p| p.origin.clone()),
        Some(Origin::file("config/app.toml"))
    );
}

#[test]
fn every_load_starts_with_an_empty_report() {
    let loader = loader(None);
    let _: AppConfig = loader.load_config().unwrap();
    let _: AppConfig = loader.load_config().unwrap();
    assert_eq!(loader.load_report().len(), 1, "warnings must not stack");

    // A failed load also starts from an empty report: the type error stops the load
    // before unknown keys are reported, so the earlier warning must be gone.
    let failing = Loader {
        env: Some(env_layer("not a port")),
        ..loader
    };
    assert_eq!(failing.load_report().len(), 1);
    assert!(CanLoadConfig::<AppConfig>::load_config(&failing).is_err());
    assert!(failing.load_report().is_empty());
    failing.load_report().push(LoadWarning::MissingProfile {
        profile: "stale".into(),
    });
    assert!(CanLoadConfig::<AppConfig>::load_config(&failing).is_err());
    assert!(failing.load_report().is_empty());
}

#[test]
fn deny_unknown_keys_reports_every_unknown_key() {
    let mut base = file_layer();
    base.as_map_mut().unwrap().insert(
        "nmae",
        ConfigValue::from("typo").with_origin(Origin::file("config/app.toml")),
    );
    let strict = StrictLoader {
        base: Some(base),
        env: None,
        load_limits: LoadLimits::DEFAULT,
    };
    let error = CanLoadConfig::<AppConfig>::load_config(&strict).unwrap_err();
    let config_error = error.downcast_ref::<ConfigError>().unwrap();
    assert_eq!(config_error.code(), "config::unknown");
    let ConfigError::Unknown { keys } = config_error else {
        panic!("unexpected error {config_error:?}");
    };
    let paths: Vec<String> = keys.iter().map(|k| k.path.to_string()).collect();
    assert_eq!(paths, ["http.prot", "nmae"]);
    assert_eq!(
        error.to_string(),
        "unknown keys (2): http.prot at config/app.toml (did you mean \"port\"?); \
         nmae at config/app.toml (did you mean \"name\"?)"
    );
}

#[test]
fn json_array_text_obeys_the_context_limits() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Tags {
        tags: Vec<String>,
    }
    let strict = StrictLoader {
        base: None,
        env: None,
        load_limits: LoadLimits::DEFAULT.with_max_keys(3),
    };
    let tree = ConfigValue::map([("tags", ConfigValue::from(r#"["a","b","c"]"#))]);
    let error = CanDeserializeConfig::<Tags>::deserialize_config(&strict, tree).unwrap_err();
    assert_eq!(
        error.downcast_ref::<ConfigError>(),
        Some(&ConfigError::limit_exceeded("max_keys", 3))
    );
    let tree = ConfigValue::map([("tags", ConfigValue::from(r#"["a","b"]"#))]);
    let tags: Tags = CanDeserializeConfig::<Tags>::deserialize_config(&strict, tree).unwrap();
    assert_eq!(tags.tags, ["a", "b"]);
}
