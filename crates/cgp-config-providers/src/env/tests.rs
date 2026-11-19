use cgp_config_core::{ConfigError, ConfigMeta, EnvSnapshot, FieldMeta, NestedMeta, NestedShape};

use super::env_layer;

fn http() -> &'static ConfigMeta {
    const META: &ConfigMeta = &ConfigMeta::new(
        "Http",
        &[
            FieldMeta::new("port", "port").with_env_raw("PORT"),
            FieldMeta::new("bind_address", "bindAddress"),
        ],
    );
    META
}

fn pool() -> &'static ConfigMeta {
    const META: &ConfigMeta = &ConfigMeta::new("Pool", &[FieldMeta::new("size", "size")]);
    META
}

fn common() -> &'static ConfigMeta {
    const META: &ConfigMeta =
        &ConfigMeta::new("Common", &[FieldMeta::new("log_level", "log_level")]);
    META
}

fn root() -> &'static ConfigMeta {
    const META: &ConfigMeta = &ConfigMeta::new(
        "App",
        &[
            FieldMeta::new("http", "http").with_nested(NestedMeta::new(NestedShape::Single, http)),
            FieldMeta::new("pools", "pools").with_nested(NestedMeta::new(NestedShape::Map, pool)),
            FieldMeta::new("common", "common")
                .with_flatten(true)
                .with_nested(NestedMeta::new(NestedShape::Single, common)),
            FieldMeta::new("token", "token").with_env("SECRET_TOKEN"),
        ],
    );
    META
}

fn get(tree: &cgp_config_value::ConfigValue, path: &str) -> Option<String> {
    tree.get_path(&path.into())
        .and_then(|value| value.as_str().map(str::to_owned))
}

#[test]
fn schema_aware_mapping() {
    let env = EnvSnapshot::from_iter([
        ("APP__HTTP__BINDADDRESS", "0.0.0.0"),
        ("APP__POOLS__Main__SIZE", "4"),
        ("APP__LOG_LEVEL", "debug"),
        ("APP__SECRET_TOKEN", "t"),
        ("APP__UNKNOWN__DEEP", "x"),
        ("APP_PROFILE", "prod"),
        ("APP____BROKEN", "x"),
        ("OTHER__HTTP__PORT", "1"),
    ]);
    let tree = env_layer(&env, "APP", Some(root())).unwrap().unwrap();
    assert_eq!(get(&tree, "http.bindAddress").as_deref(), Some("0.0.0.0"));
    assert_eq!(get(&tree, "pools.main.size").as_deref(), Some("4"));
    assert_eq!(
        get(&tree, "common.log_level").as_deref(),
        Some("debug"),
        "flattened field, canonical"
    );
    assert_eq!(
        get(&tree, "token").as_deref(),
        Some("t"),
        "env override name"
    );
    assert_eq!(get(&tree, "unknown.deep").as_deref(), Some("x"));
    assert!(tree.get("profile").is_none());
    let origin = tree
        .get_path(&"http.bindAddress".into())
        .unwrap()
        .meta()
        .origin
        .to_string();
    assert!(origin.contains("APP__HTTP__BINDADDRESS"), "{origin}");
}

#[test]
fn values_stay_strings_and_lists_are_not_split() {
    let env = EnvSnapshot::from_iter([("APP__HOSTS", "a,b"), ("APP__N", "5")]);
    let tree = env_layer(&env, "APP", None).unwrap().unwrap();
    assert_eq!(get(&tree, "hosts").as_deref(), Some("a,b"));
    assert_eq!(get(&tree, "n").as_deref(), Some("5"));
}

#[test]
fn env_raw_is_read_and_prefixed_wins() {
    let env = EnvSnapshot::from_iter([("PORT", "8080")]);
    let tree = env_layer(&env, "APP", Some(root())).unwrap().unwrap();
    assert_eq!(get(&tree, "http.port").as_deref(), Some("8080"));

    let env = EnvSnapshot::from_iter([("PORT", "8080"), ("APP__HTTP__PORT", "9090")]);
    let tree = env_layer(&env, "APP", Some(root())).unwrap().unwrap();
    assert_eq!(get(&tree, "http.port").as_deref(), Some("9090"));
    assert!(env_layer(&env, "APP", None).unwrap().is_some());
    assert!(
        env_layer(&EnvSnapshot::new(), "APP", Some(root()))
            .unwrap()
            .is_none()
    );
}

#[test]
fn collisions_are_ambiguous() {
    let env = EnvSnapshot::from_iter([("APP__A", "1"), ("APP__A__B", "2")]);
    let error = env_layer(&env, "APP", None).unwrap_err();
    assert!(
        matches!(error, ConfigError::AmbiguousEnv { ref key, .. } if key == "APP__A__B"),
        "{error}"
    );

    const CASES: &ConfigMeta = &ConfigMeta::new(
        "Cases",
        &[
            FieldMeta::new("a", "maxConn"),
            FieldMeta::new("b", "maxconn"),
        ],
    );
    let env = EnvSnapshot::from_iter([("APP__MAXCONN", "1")]);
    let error = env_layer(&env, "APP", Some(CASES)).unwrap_err();
    let ConfigError::AmbiguousEnv { candidates, .. } = error else {
        panic!("expected AmbiguousEnv");
    };
    assert_eq!(candidates.len(), 2);
}
