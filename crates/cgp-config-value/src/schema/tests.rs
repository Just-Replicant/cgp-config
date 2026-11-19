use alloc::string::{String, ToString};
use alloc::vec::Vec;

use cgp_config_core::{
    ConfigMeta, ConfigSection, ConstDefault, FieldMeta, KeyPath, LoadWarning, Meta, NestedMeta,
    NestedShape, Origin, ParseKind, UnknownKey,
};

use super::{defaults_tree, normalize, split_denied};
use crate::ConfigValue;

struct Tls;
impl ConfigSection for Tls {
    const META: &'static ConfigMeta = &ConfigMeta::new(
        "Tls",
        &[
            FieldMeta::new("cert", "cert")
                .with_required(true)
                .with_type("String"),
            FieldMeta::new("key", "key").with_secret(true),
        ],
    )
    .with_deny_unknown(true);
}

struct Http;
impl ConfigSection for Http {
    const META: &'static ConfigMeta = &ConfigMeta::new(
        "Http",
        &[
            FieldMeta::new("port", "port")
                .with_default(ConstDefault::UInt(8080))
                .with_aliases(&["listen_port"]),
            FieldMeta::new("timeout", "timeout")
                .with_default(ConstDefault::Str("30s"))
                .with_parse(ParseKind::Duration),
            FieldMeta::new("max_body", "max_body").with_parse(ParseKind::Bytes),
            FieldMeta::new("tls", "tls")
                .with_optional(true)
                .with_nested(NestedMeta::new(
                    NestedShape::Single,
                    <Tls as ConfigSection>::meta,
                )),
        ],
    );
}

struct Common;
impl ConfigSection for Common {
    const META: &'static ConfigMeta = &ConfigMeta::new(
        "Common",
        &[FieldMeta::new("log_level", "log_level").with_default(ConstDefault::Str("info"))],
    );
}

struct App;
impl ConfigSection for App {
    const META: &'static ConfigMeta = &ConfigMeta::new(
        "App",
        &[
            FieldMeta::new("http", "http").with_nested(NestedMeta::new(
                NestedShape::Single,
                <Http as ConfigSection>::meta,
            )),
            FieldMeta::new("common", "common")
                .with_flatten(true)
                .with_nested(NestedMeta::new(
                    NestedShape::Single,
                    <Common as ConfigSection>::meta,
                )),
            FieldMeta::new("name", "name").with_doc_key("app-name"),
            FieldMeta::new("old", "old").with_deprecated("use `name`"),
            FieldMeta::new("backends", "backends").with_nested(NestedMeta::new(
                NestedShape::Seq,
                <Tls as ConfigSection>::meta,
            )),
        ],
    );
}

fn run(
    tree: ConfigValue,
) -> (
    Result<ConfigValue, cgp_config_core::ConfigError>,
    Vec<LoadWarning>,
) {
    let mut warnings = Vec::new();
    let result = normalize(tree, App::META, &mut |w| warnings.push(w));
    (result, warnings)
}

#[test]
fn defaults_are_canonical() {
    let tree = defaults_tree(App::META);
    let http = tree.get("http").unwrap();
    assert_eq!(http.get("port").unwrap().as_u64(), Some(8080));
    assert_eq!(http.get("timeout").unwrap().as_str(), Some("30s"));
    assert!(
        http.get("tls").is_none(),
        "optional sections get no defaults"
    );
    let common = tree.get("common").unwrap();
    assert_eq!(
        common.get("log_level").unwrap().as_str(),
        Some("info"),
        "flattened under its field"
    );
    assert!(tree.get("log_level").is_none());
    assert_eq!(tree.get("http").unwrap().meta().origin, Origin::Defaults);
}

#[test]
fn canonicalize_renames_per_layer_only() {
    let layer = ConfigValue::map([
        (
            "http",
            ConfigValue::map([("listen_port", ConfigValue::from(1))]),
        ),
        ("log_level", ConfigValue::from("debug")),
        ("app-name", ConfigValue::from("svc")),
        ("old", ConfigValue::from("x")),
    ]);
    let mut warnings = Vec::new();
    let tree = super::canonicalize(layer, App::META, &mut |w| warnings.push(w)).unwrap();
    assert_eq!(
        tree.get_path(&"http.port".into()).unwrap().as_u64(),
        Some(1)
    );
    assert_eq!(
        tree.get_path(&"common.log_level".into()).unwrap().as_str(),
        Some("debug")
    );
    assert_eq!(tree.get("name").unwrap().as_str(), Some("svc"));
    // Only the alias warns here; `deprecated` fields and parsers wait for `normalize`.
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(tree.get("name").is_some() && tree.get("http").unwrap().get("timeout").is_none());

    let both = ConfigValue::map([(
        "http",
        ConfigValue::map([
            ("listen_port", ConfigValue::from(1)),
            ("port", ConfigValue::from(2)),
        ]),
    )]);
    let error = super::canonicalize(both, App::META, &mut |_| {}).unwrap_err();
    assert_eq!(error.code(), "config::duplicate_key");
}

#[test]
fn an_alias_overrides_a_default_under_the_current_name() {
    let mut defaults = defaults_tree(App::META);
    let mut overlay = ConfigValue::map([
        (
            "http",
            ConfigValue::map([("listen_port", ConfigValue::from("9"))]),
        ),
        ("name", ConfigValue::from("x")),
    ]);
    overlay.stamp_origin(&Origin::cli("--set"));
    defaults = crate::merge::merge(defaults, overlay, &crate::merge::MergeOptions::DEEP).unwrap();
    let (result, _) = run(defaults);
    let tree = result.unwrap();
    assert_eq!(
        tree.get_path(&"http.port".into()).unwrap().as_str(),
        Some("9")
    );
}

#[test]
fn normalizes_aliases_mounts_flatten_and_units() {
    let tree = ConfigValue::map([
        (
            "http",
            ConfigValue::map([
                ("listen_port", ConfigValue::from("9000")),
                ("timeout", ConfigValue::from("1m30s")),
                ("max_body", ConfigValue::from("1KiB")),
            ]),
        ),
        ("log_level", ConfigValue::from("debug")),
        ("app-name", ConfigValue::from("svc")),
        ("old", ConfigValue::from("x")),
        ("backends", ConfigValue::array([])),
    ]);
    let (result, warnings) = run(tree);
    let tree = result.unwrap();
    let http = tree.get("http").unwrap();
    assert_eq!(http.get("port").unwrap().as_str(), Some("9000"));
    let timeout = http.get("timeout").unwrap();
    assert_eq!(timeout.get("secs").unwrap().as_u64(), Some(90));
    assert_eq!(timeout.get("nanos").unwrap().as_u64(), Some(0));
    assert_eq!(http.get("max_body").unwrap().as_u64(), Some(1024));
    assert_eq!(
        tree.get("common")
            .unwrap()
            .get("log_level")
            .unwrap()
            .as_str(),
        Some("debug")
    );
    assert!(tree.get("log_level").is_none());
    assert_eq!(tree.get("name").unwrap().as_str(), Some("svc"));
    let codes: Vec<_> = warnings.iter().map(LoadWarning::code).collect();
    assert_eq!(codes, ["config::warn::deprecated_key"; 2]);
}

#[test]
fn every_failure_is_reported_together() {
    let tree = ConfigValue::map([
        (
            "http",
            ConfigValue::map([
                ("port", ConfigValue::from(1)),
                ("listen_port", ConfigValue::from(2)),
                ("timeout", ConfigValue::from(30)),
                ("max_body", ConfigValue::from("1.5")),
                ("tls", ConfigValue::map([("key", ConfigValue::from("k"))])),
            ]),
        ),
        (
            "backends",
            ConfigValue::array([
                ConfigValue::map([("cert", ConfigValue::from("c"))]),
                ConfigValue::empty_map(),
            ]),
        ),
    ]);
    let (result, _) = run(tree);
    let error = result.unwrap_err();
    let cgp_config_core::ConfigError::Validation(errors) = error else {
        panic!("expected aggregated errors, got {error:?}");
    };
    let found: Vec<_> = errors
        .iter()
        .map(|issue| (issue.path.to_string(), issue.code))
        .collect();
    assert_eq!(
        found,
        [
            (String::from("http.port"), "config::duplicate_key"),
            ("http.timeout".into(), "config::type"),
            ("http.max_body".into(), "config::type"),
            ("http.tls.cert".into(), "config::missing"),
            ("backends[1].cert".into(), "config::missing"),
        ]
    );
}

#[test]
fn a_bare_number_is_not_a_duration_and_secrets_are_marked() {
    let tree = ConfigValue::map([(
        "http",
        ConfigValue::map([
            ("timeout", ConfigValue::from("30")),
            (
                "tls",
                ConfigValue::map([("cert", "c".into()), ("key", "k".into())]),
            ),
        ]),
    )]);
    let (result, _) = run(tree.clone());
    let error = result.unwrap_err();
    assert_eq!(error.code(), "config::type");
    assert!(error.to_string().contains("needs a unit"), "{error}");

    let mut fixed = tree;
    let http = fixed.as_map_mut().unwrap().get_mut("http").unwrap();
    http.as_map_mut()
        .unwrap()
        .insert("timeout", ConfigValue::from("30s"));
    let tree = run(fixed).0.unwrap();
    let key = tree
        .get_path(&KeyPath::parse("http.tls.key").unwrap())
        .unwrap();
    assert!(key.meta().secret);
}

#[test]
fn deny_unknown_applies_per_section() {
    let unknown = ["http.tls.extra", "http.extra", "backends[0].typo", "extra"]
        .map(|path| UnknownKey::new(KeyPath::parse(path).unwrap(), None, Meta::default()));
    let (denied, rest) = split_denied(App::META, unknown.to_vec());
    let denied: Vec<_> = denied.iter().map(|key| key.path.to_string()).collect();
    let rest: Vec<_> = rest.iter().map(|key| key.path.to_string()).collect();
    assert_eq!(denied, ["http.tls.extra", "backends[0].typo"]);
    assert_eq!(rest, ["http.extra", "extra"]);
}
