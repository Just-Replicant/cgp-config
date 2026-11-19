//! `#[derive(ConfigSection)]` end to end through the facade: metadata, defaults, env
//! mapping, flatten, per-section `deny_unknown`, parsers, aggregated errors, and
//! interpolation of opted-in fields only.

#![cfg(all(feature = "std", feature = "derive", feature = "toml"))]
#![allow(
    clippy::std_instead_of_core,
    clippy::std_instead_of_alloc,
    missing_docs
)]

use std::collections::BTreeMap;
use std::time::Duration;

use cgp_config::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize, ConfigSection, PartialEq)]
pub struct Logging {
    #[config(default = "info")]
    level: String,
}

#[derive(Debug, Deserialize, ConfigSection, PartialEq)]
#[config(deny_unknown)]
pub struct Pool {
    #[config(default = 4)]
    size: u32,
    #[config(default = "64KiB", parse = "bytes")]
    buffer: u64,
}

#[derive(Debug, Deserialize, ConfigSection, PartialEq)]
pub struct Server {
    host: String,
    #[config(default = 80)]
    port: u16,
}

/// The root.
#[derive(Debug, Deserialize, ConfigSection, PartialEq)]
#[serde(rename_all = "camelCase")]
#[config(env_prefix = "APP")]
pub struct App {
    #[config(alias = "app_name")]
    name: String,
    #[config(default = "30s", parse = "duration")]
    request_timeout: Duration,
    #[config(flatten)]
    logging: Logging,
    #[config(nested)]
    #[serde(default)]
    pools: BTreeMap<String, Pool>,
    #[config(nested)]
    #[serde(default)]
    servers: Vec<Server>,
    #[config(secret)]
    token: Option<String>,
    #[config(interpolate, default = "${HOME_DIR}/data")]
    data_dir: String,
    #[config(default = "${NOT_EXPANDED}")]
    motto: String,
}

#[test]
fn meta_describes_the_struct() {
    let meta = App::META;
    assert_eq!(meta.doc, "The root.");
    assert_eq!(meta.env_prefix, Some("APP"));
    let timeout = meta.field("requestTimeout").unwrap();
    assert_eq!(timeout.name, "request_timeout");
    assert_eq!(timeout.ty, "Duration");
    assert!(meta.field("name").unwrap().required);
    assert!(!meta.field("token").unwrap().required);
    assert!(meta.field("logging").unwrap().flatten);
    assert!(Pool::META.deny_unknown);
}

#[test]
fn defaults_flatten_maps_lists_and_parsers() {
    let config: App = cgp_config::from_toml_str(
        r#"
        app_name = "svc"
        level = "debug"
        requestTimeout = "1m30s"
        [pools.main]
        buffer = "1MiB"
        [[servers]]
        host = "a"
        "#,
    )
    .unwrap();
    assert_eq!(config.name, "svc", "alias");
    assert_eq!(config.logging.level, "debug", "flattened key on the parent");
    assert_eq!(config.request_timeout, Duration::from_secs(90));
    assert_eq!(
        config.pools["main"],
        Pool {
            size: 4,
            buffer: 1 << 20
        }
    );
    assert_eq!(
        config.servers,
        [Server {
            host: "a".into(),
            port: 80
        }]
    );
    assert_eq!(
        config.motto, "${NOT_EXPANDED}",
        "not opted in: kept literally"
    );
    assert_eq!(
        config.data_dir, "${HOME_DIR}/data",
        "interpolation is off by default"
    );
}

#[test]
fn errors_are_aggregated() {
    let error = cgp_config::from_toml_str::<App>(
        "requestTimeout = 30\n[pools.main]\nsize = 1\n[[servers]]\nport = 1\n",
    )
    .unwrap_err();
    let ConfigError::Validation(issues) = &error else {
        panic!("expected every problem at once, got {error}");
    };
    assert_eq!(issues.len(), 3, "{error}");
    let text = error.to_string();
    for expected in ["name", "unit", "servers[0].host", "toml:1:"] {
        assert!(text.contains(expected), "`{expected}` missing from: {text}");
    }

    // Unknown keys inside a `deny_unknown` section fail; elsewhere they only warn.
    let error =
        cgp_config::from_toml_str::<App>("name = \"x\"\n[pools.main]\nextra = true\n").unwrap_err();
    assert!(error.to_string().contains("pools.main.extra"), "{error}");
    let loader = MemoryLoader::from_toml::<App>("name = \"x\"\nother = 1\n").unwrap();
    let _: App = loader.load().unwrap();
    assert!(
        loader
            .load_report
            .warnings()
            .iter()
            .any(|w| w.to_string().contains("other"))
    );
}

#[test]
fn from_defaults_and_json() {
    let error = App::from_defaults().unwrap_err();
    assert!(error.to_string().contains("name"), "{error}");
    let logging = Logging::from_defaults().unwrap();
    assert_eq!(logging.level, "info");
    #[cfg(feature = "json")]
    {
        let app: App = cgp_config::from_json_str(r#"{"name": "j", "pools": {}}"#).unwrap();
        assert_eq!(app.name, "j");
    }
}

#[test]
fn env_maps_with_the_schema_and_aliases_merge_by_precedence() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.toml"),
        "app_name = \"file\"\n[pools.main]\nsize = 2\n",
    )
    .unwrap();
    let boot = Bootstrap::new("app")
        .with_env(EnvSnapshot::from_iter([
            ("APP__NAME", "env"),
            ("APP__REQUESTTIMEOUT", "5s"),
            ("APP__LEVEL", "warn"),
            ("APP__POOLS__Main__BUFFER", "2KiB"),
            ("APP__TOKEN", "t0p"),
        ]))
        .search(SearchPath::new("app").with_working_dir(Some(dir.path().to_path_buf())))
        .finish();
    let loader = StdLoader::for_section::<App>(boot);
    let config: App = loader.load().unwrap();
    assert_eq!(
        config.name, "env",
        "env (current name) beats the file's alias"
    );
    assert_eq!(config.request_timeout, Duration::from_secs(5));
    assert_eq!(config.logging.level, "warn");
    assert_eq!(
        config.pools["main"],
        Pool {
            size: 2,
            buffer: 2048
        }
    );
    assert_eq!(config.token.as_deref(), Some("t0p"));
    let warnings = loader.load_report.warnings();
    assert!(
        warnings.iter().any(
            |w| matches!(w, LoadWarning::DeprecatedKey { alias, .. } if &**alias == "app_name")
        ),
        "{warnings:?}"
    );
}

#[derive(Debug, Deserialize, ConfigSection)]
pub struct Wrapper<T> {
    #[config(nested)]
    inner: T,
}

#[test]
fn generic_sections() {
    let meta = <Wrapper<Logging> as ConfigSection>::META;
    let inner = meta.field("inner").unwrap().single_section().unwrap();
    assert_eq!(inner.name, "Logging");
    let config: Wrapper<Logging> = cgp_config::from_toml_str("").unwrap();
    assert_eq!(config.inner.level, "info");
}

// ---- Interpolation (feature `interpolate`) -----------------------------------------

#[cfg(feature = "interpolate")]
mod interpolation {
    use cgp_config::cgp::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
    use cgp_config::cgp::extra::error::RaiseFrom;

    use super::*;

    #[derive(HasField)]
    struct Loader {
        env_snapshot: EnvSnapshot,
        env_prefix: String,
        active_profile: String,
        config_location: ConfigLocation,
        load_report: LoadReport,
    }

    delegate_components! {
        #[unchecked]
        Loader {
            preset InterpolatingPreset,
            ErrorTypeProviderComponent: UseType<ConfigError>,
            ErrorRaiserComponent: RaiseFrom,
            RootConfigMetaComponent: UseConfigMeta<App>,
            ConfigValidatorComponent: Nested,
        }
    }

    #[test]
    fn only_opted_in_fields_expand() {
        let boot = Bootstrap::new("app")
            .with_env(EnvSnapshot::from_iter([
                ("HOME_DIR", "/srv"),
                ("NOT_EXPANDED", "boom"),
                ("APP__NAME", "n"),
            ]))
            .search(SearchPath::new("app"))
            .finish();
        let loader = Loader {
            env_snapshot: boot.env_snapshot,
            env_prefix: boot.env_prefix,
            active_profile: boot.active_profile,
            config_location: boot.config_location,
            load_report: LoadReport::new(),
        };
        let config: App = loader.load_config().unwrap();
        assert_eq!(config.data_dir, "/srv/data");
        assert_eq!(config.motto, "${NOT_EXPANDED}");
    }
}
