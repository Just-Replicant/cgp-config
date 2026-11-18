//! The provenance-aware deserializer: coercions, errors, unknown keys, secrets.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

use std::collections::BTreeMap;

use cgp_config_core::{ConfigError, Meta, Origin};
use cgp_config_value::{ConfigValue, from_value, from_value_with_unknown};
use serde::Deserialize;

fn env(key: &str, text: &str) -> ConfigValue {
    ConfigValue::from(text).with_origin(Origin::env(key))
}

fn err<T: serde::de::DeserializeOwned + std::fmt::Debug>(value: &ConfigValue) -> ConfigError {
    from_value::<T>(value).unwrap_err()
}

#[test]
fn native_values_deserialize_directly() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct All {
        b: bool,
        u: u8,
        i: i64,
        f: f32,
        s: String,
        c: char,
        v: Vec<u16>,
        t: (u8, String),
        o: Option<u8>,
        n: Option<u8>,
        unit: (),
    }
    let tree = ConfigValue::map([
        ("b", true.into()),
        ("u", 255.into()),
        ("i", (-5).into()),
        ("f", 1.5.into()),
        ("s", "hi".into()),
        ("c", "x".into()),
        ("v", ConfigValue::array([1.into(), 2.into()])),
        ("t", ConfigValue::array([1.into(), "a".into()])),
        ("o", 3.into()),
        ("n", ConfigValue::null()),
        ("unit", ConfigValue::null()),
    ]);
    assert_eq!(
        from_value::<All>(&tree).unwrap(),
        All {
            b: true,
            u: 255,
            i: -5,
            f: 1.5,
            s: "hi".into(),
            c: 'x',
            v: vec![1, 2],
            t: (1, "a".into()),
            o: Some(3),
            n: None,
            unit: (),
        }
    );
}

#[test]
fn strings_coerce_to_scalars() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Env {
        a: bool,
        b: bool,
        c: bool,
        d: bool,
        port: u16,
        neg: i32,
        ratio: f64,
        big: u128,
        opt: Option<u32>,
    }
    let tree = ConfigValue::map([
        ("a", "true".into()),
        ("b", "FALSE".into()),
        ("c", "True".into()),
        ("d", "false".into()),
        ("port", "8080".into()),
        ("neg", "-12".into()),
        ("ratio", "0.25".into()),
        ("big", "340282366920938463463374607431768211455".into()),
        ("opt", "7".into()),
    ]);
    let parsed: Env = from_value(&tree).unwrap();
    assert!(parsed.a && !parsed.b && parsed.c && !parsed.d);
    assert_eq!(parsed.port, 8080);
    assert_eq!(parsed.neg, -12);
    assert!((parsed.ratio - 0.25).abs() < f64::EPSILON);
    assert_eq!(parsed.big, u128::MAX);
    assert_eq!(parsed.opt, Some(7));
    // Only true/false (any case) and the native bool; no yes/no/1/0 guessing.
    for text in ["yes", "1", "on", "", " true"] {
        assert_eq!(
            err::<bool>(&ConfigValue::from(text)).code(),
            "config::type",
            "{text:?}"
        );
    }
}

#[test]
fn integers_widen_to_floats_but_not_back() {
    assert!((from_value::<f64>(&ConfigValue::from(3)).unwrap() - 3.0).abs() < f64::EPSILON);
    let error = err::<u8>(&ConfigValue::from(1.5));
    assert_eq!(error.to_string(), "expected u8, found float 1.5");
}

#[test]
fn out_of_range_numbers_are_type_errors() {
    let tree = ConfigValue::map([(
        "port",
        ConfigValue::from(70_000).with_origin(Origin::file("app.toml")),
    )]);
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Http {
        port: u16,
    }
    let error = err::<Http>(&tree);
    assert_eq!(error.code(), "config::type");
    assert_eq!(
        error.to_string(),
        "port: expected u16, found 70000 (out of range) at app.toml"
    );
    assert_eq!(
        err::<u8>(&ConfigValue::from(-1)).to_string(),
        "expected u8, found -1 (out of range)"
    );
    assert_eq!(err::<u8>(&env("X", "300")).code(), "config::type");
}

#[test]
fn bad_strings_name_the_target_and_input() {
    let error = err::<u16>(&env("APP__PORT", "http"));
    assert_eq!(
        error.to_string(),
        "expected u16, found string \"http\" at env APP__PORT"
    );
    assert_eq!(error.meta().unwrap().origin, Origin::env("APP__PORT"));
    let error = err::<bool>(&env("APP__TLS", "maybe"));
    assert!(error.to_string().starts_with("expected bool"), "{error}");
    assert!(
        err::<char>(&ConfigValue::from("ab"))
            .to_string()
            .contains("char")
    );
}

#[test]
fn wrong_kinds_are_type_errors_with_paths() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Outer {
        servers: Vec<Server>,
    }
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Server {
        name: String,
    }
    let tree = ConfigValue::map([(
        "servers",
        ConfigValue::array([
            ConfigValue::map([("name", "a".into())]),
            ConfigValue::map([("name", ConfigValue::map([("x", 1.into())]))]),
        ]),
    )]);
    let error = err::<Outer>(&tree);
    assert_eq!(error.path().unwrap().to_string(), "servers[1].name");
    assert!(
        error
            .to_string()
            .starts_with("servers[1].name: invalid type: map"),
        "{error}"
    );
}

#[test]
fn missing_fields_report_the_full_path() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Outer {
        db: Db,
    }
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Db {
        url: String,
        pool: u8,
    }
    let error = err::<Outer>(&ConfigValue::map([(
        "db",
        ConfigValue::map([("url", "x".into())]),
    )]));
    assert_eq!(error.code(), "config::missing");
    assert_eq!(error.path().unwrap().to_string(), "db.pool");
    assert_eq!(error.to_string(), "missing db.pool");
}

#[test]
fn defaults_and_options_fill_absent_keys() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct WithDefaults {
        #[serde(default)]
        workers: u8,
        name: Option<String>,
    }
    assert_eq!(
        from_value::<WithDefaults>(&ConfigValue::empty_map()).unwrap(),
        WithDefaults {
            workers: 0,
            name: None
        }
    );
}

#[test]
fn unknown_keys_are_collected_with_suggestions_and_origins() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Http {
        port: u16,
        host: String,
    }
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct App {
        http: Http,
    }
    let tree = ConfigValue::map([
        (
            "http",
            ConfigValue::map([
                ("port", 1.into()),
                ("host", "h".into()),
                ("hots", "typo".into()),
                (
                    "completely_different",
                    ConfigValue::map([("nested", 1.into())]),
                ),
            ]),
        ),
        ("extra", 1.into()),
    ])
    .with_meta(Meta::default());
    let mut tree = tree;
    tree.stamp_origin(&Origin::file("app.toml"));
    let (_, unknown) = from_value_with_unknown::<App>(&tree).unwrap();
    let found: Vec<_> = unknown
        .iter()
        .map(|u| {
            (
                u.path.to_string(),
                u.suggestion.clone(),
                u.meta.origin.to_string(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (
                "http.hots".to_owned(),
                Some("host".to_owned()),
                "app.toml".to_owned()
            ),
            (
                "http.completely_different".to_owned(),
                None,
                "app.toml".to_owned()
            ),
            ("extra".to_owned(), None, "app.toml".to_owned()),
        ]
    );
    // Plain `from_value` ignores them.
    assert!(from_value::<App>(&tree).is_ok());
}

#[test]
fn flattened_maps_absorb_unknown_keys() {
    #[derive(Deserialize, Debug)]
    struct Open {
        #[allow(dead_code)]
        known: u8,
        #[serde(flatten)]
        rest: BTreeMap<String, u8>,
    }
    let tree = ConfigValue::map([("known", 1.into()), ("other", 2.into())]);
    let (open, unknown) = from_value_with_unknown::<Open>(&tree).unwrap();
    assert_eq!(open.rest.get("other"), Some(&2));
    assert!(unknown.is_empty());
}

#[test]
fn deny_unknown_fields_becomes_an_unknown_error() {
    #[derive(Deserialize, Debug)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Strict {
        port: u16,
    }
    let tree = ConfigValue::map([
        ("port", 1.into()),
        (
            "prot",
            ConfigValue::from(2).with_origin(Origin::cli("--prot")),
        ),
    ]);
    let error = err::<Strict>(&tree);
    assert_eq!(error.code(), "config::unknown");
    assert_eq!(
        error.to_string(),
        "unknown key prot at cli --prot; did you mean \"port\"?"
    );
}

#[test]
fn enums_accept_strings_and_maps_and_suggest_variants() {
    #[derive(Deserialize, Debug, PartialEq)]
    #[serde(rename_all = "lowercase")]
    enum Level {
        Debug,
        Info,
        Warn,
    }
    #[derive(Deserialize, Debug, PartialEq)]
    enum Shape {
        Circle(f64),
        Rect { w: u8, h: u8 },
        Pair(u8, u8),
        Unit,
    }
    assert_eq!(from_value::<Level>(&"info".into()).unwrap(), Level::Info);
    let error = err::<Level>(&env("APP__LEVEL", "inf"));
    assert_eq!(
        error.to_string(),
        "unknown variant \"inf\", expected one of \"debug\", \"info\", \"warn\"; did you mean \"info\"? at env APP__LEVEL"
    );
    assert_eq!(
        from_value::<Shape>(&ConfigValue::map([("Circle", 1.0.into())])).unwrap(),
        Shape::Circle(1.0)
    );
    assert_eq!(
        from_value::<Shape>(&ConfigValue::map([(
            "Rect",
            ConfigValue::map([("w", 1.into()), ("h", "2".into())])
        )]))
        .unwrap(),
        Shape::Rect { w: 1, h: 2 }
    );
    assert_eq!(
        from_value::<Shape>(&ConfigValue::map([(
            "Pair",
            ConfigValue::array([1.into(), 2.into()])
        )]))
        .unwrap(),
        Shape::Pair(1, 2)
    );
    assert_eq!(from_value::<Shape>(&"Unit".into()).unwrap(), Shape::Unit);
    assert_eq!(
        err::<Shape>(&ConfigValue::map([
            ("Unit", 1.into()),
            ("Circle", 1.into())
        ]))
        .code(),
        "config::type"
    );
    assert_eq!(err::<Shape>(&ConfigValue::from(1)).code(), "config::type");
    let _ = Level::Debug;
    let _ = Level::Warn;
}

#[test]
fn json_array_text_becomes_a_sequence_with_the_strings_origin() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Hosts {
        ports: Vec<u16>,
    }
    let ok: Vec<String> = from_value(&env("APP__HOSTS", r#"["a","b"]"#)).unwrap();
    assert_eq!(ok, ["a", "b"]);
    let error = err::<Hosts>(&ConfigValue::map([(
        "ports",
        env("APP__PORTS", "[1, 70000]"),
    )]));
    assert_eq!(error.path().unwrap().to_string(), "ports[1]");
    assert_eq!(error.meta().unwrap().origin, Origin::env("APP__PORTS"));
    let error = err::<Vec<u8>>(&env("APP__V", "1,2"));
    assert!(error.to_string().contains("JSON array text"), "{error}");
    let error = err::<Vec<u8>>(&env("APP__V", "[1,"));
    assert!(
        error.to_string().contains("invalid JSON array text"),
        "{error}"
    );
}

#[test]
fn map_keys_parse_into_typed_keys() {
    let tree = ConfigValue::map([("1", "a".into()), ("20", "b".into())]);
    let parsed: BTreeMap<u16, String> = from_value(&tree).unwrap();
    assert_eq!(parsed.get(&20).map(String::as_str), Some("b"));
    let flags: BTreeMap<bool, u8> = from_value(&ConfigValue::map([("true", 1.into())])).unwrap();
    assert_eq!(flags.get(&true), Some(&1));
    assert_eq!(
        err::<BTreeMap<u8, u8>>(&ConfigValue::map([("x", 1.into())])).code(),
        "config::type"
    );
}

#[test]
fn secret_values_are_never_echoed() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Db {
        password: u32,
    }
    let secret = ConfigValue::from("hunter2")
        .with_meta(Meta::new(Origin::env("APP__DB__PASSWORD")).with_secret(true));
    let error = err::<Db>(&ConfigValue::map([("password", secret.clone())]));
    let text = error.to_string();
    assert!(!text.contains("hunter2"), "{text}");
    assert_eq!(
        text,
        "password: invalid secret value at env APP__DB__PASSWORD"
    );
    assert!(!format!("{error:?}").contains("hunter2"));
    assert!(!format!("{secret:?}").contains("hunter2"));

    // Secrets inside JSON text stay secret element-wise.
    let list = ConfigValue::from("[\"s3cret\", 1]")
        .with_meta(Meta::new(Origin::env("K")).with_secret(true));
    let error = err::<Vec<String>>(&list);
    assert!(!error.to_string().contains("s3cret"), "{error}");
    let enum_error = err::<Option<bool>>(
        &ConfigValue::from("s3cret").with_meta(Meta::default().with_secret(true)),
    );
    assert!(!enum_error.to_string().contains("s3cret"));
}

#[test]
fn bytes_and_borrowed_strings() {
    #[derive(Deserialize, Debug)]
    struct Raw {
        #[serde(with = "serde_bytes_like")]
        data: Vec<u8>,
    }
    mod serde_bytes_like {
        pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
            d: D,
        ) -> Result<Vec<u8>, D::Error> {
            struct V;
            impl serde::de::Visitor<'_> for V {
                type Value = Vec<u8>;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("bytes")
                }
                fn visit_bytes<E>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                    Ok(v.to_vec())
                }
                fn visit_str<E>(self, v: &str) -> Result<Vec<u8>, E> {
                    Ok(v.as_bytes().to_vec())
                }
            }
            d.deserialize_bytes(V)
        }
    }
    let raw: Raw = from_value(&ConfigValue::map([("data", "abc".into())])).unwrap();
    assert_eq!(raw.data, b"abc");
}

#[test]
fn a_value_tree_deserializes_into_itself_and_json() {
    let tree = ConfigValue::map([(
        "a",
        ConfigValue::array([1.into(), "x".into(), ConfigValue::null()]),
    )]);
    let copy: ConfigValue = from_value(&tree).unwrap();
    assert_eq!(copy, tree);
    let json: serde_json::Value = from_value(&tree).unwrap();
    assert_eq!(json, serde_json::json!({"a": [1, "x", null]}));
}

#[test]
fn ignored_values_and_newtypes() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Port(u16);
    #[derive(Deserialize, Debug, PartialEq)]
    struct Unit;
    assert_eq!(from_value::<Port>(&"80".into()).unwrap(), Port(80));
    assert_eq!(from_value::<Unit>(&ConfigValue::null()).unwrap(), Unit);
    serde::de::IgnoredAny::deserialize(&ConfigValue::from(1)).unwrap();
}
