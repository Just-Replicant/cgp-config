//! Less common serde paths: wrong kinds for every target, enum and key edge cases, and
//! the `Deserializer` impl on `&ConfigValue`.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use cgp_config_core::{Meta, Origin};
use cgp_config_value::{ConfigValue, DeserializeError, from_value};
use serde::Deserialize;
use serde::de::{self, Deserializer, IntoDeserializer, Visitor};

fn code<T: de::DeserializeOwned + fmt::Debug>(value: &ConfigValue) -> &'static str {
    from_value::<T>(value).unwrap_err().code()
}

#[test]
fn every_target_rejects_the_wrong_kind() {
    let map = ConfigValue::map([("a", 1.into())]);
    let array = ConfigValue::array([1.into()]);
    for value in [&map, &array, &ConfigValue::null()] {
        assert_eq!(code::<u8>(value), "config::type");
        assert_eq!(code::<i8>(value), "config::type");
        assert_eq!(code::<f64>(value), "config::type");
        assert_eq!(code::<bool>(value), "config::type");
        assert_eq!(code::<char>(value), "config::type");
        assert_eq!(code::<String>(value), "config::type");
    }
    assert_eq!(code::<()>(&ConfigValue::from(1)), "config::type");
    assert_eq!(code::<Vec<u8>>(&map), "config::type");
    assert_eq!(code::<Vec<u8>>(&ConfigValue::from(true)), "config::type");
    assert_eq!(code::<BTreeMap<String, u8>>(&array), "config::type");
    assert_eq!(code::<(u8, u8)>(&map), "config::type");
    assert_eq!(code::<char>(&ConfigValue::from(1)), "config::type");
    assert_eq!(code::<String>(&ConfigValue::from(1)), "config::type");

    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct S {
        a: u8,
    }
    for value in [array.clone(), ConfigValue::from(1), ConfigValue::from("x")] {
        assert_eq!(code::<S>(&value), "config::type");
    }
}

#[test]
fn invalid_type_messages_name_what_was_found() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct S {
        a: u8,
    }
    let found = |value: ConfigValue| from_value::<S>(&value).unwrap_err().to_string();
    assert!(found(ConfigValue::null()).contains("unit value"));
    assert!(found(ConfigValue::from(true)).contains("boolean `true`"));
    assert!(found(ConfigValue::from(-2)).contains("integer `-2`"));
    assert!(found(ConfigValue::from(2.5)).contains("floating point `2.5`"));
    assert!(found(ConfigValue::from("s")).contains("string \"s\""));
    assert!(found(ConfigValue::array([])).contains("sequence"));
}

#[test]
fn std_types_parse_from_strings() {
    #[derive(Deserialize, Debug)]
    struct Net {
        dir: PathBuf,
        ip: IpAddr,
        addr: SocketAddr,
    }
    let net: Net = from_value(&ConfigValue::map([
        ("dir", "/var/lib/app".into()),
        ("ip", "::1".into()),
        ("addr", "127.0.0.1:8080".into()),
    ]))
    .unwrap();
    assert_eq!(net.dir, PathBuf::from("/var/lib/app"));
    assert!(net.ip.is_loopback());
    assert_eq!(net.addr.port(), 8080);
    assert_eq!(code::<IpAddr>(&"not-an-ip".into()), "config::type");
}

#[test]
fn enum_edge_cases() {
    #[derive(Deserialize, Debug, PartialEq)]
    enum Only {
        One,
    }
    #[derive(Deserialize, Debug, PartialEq)]
    enum E {
        Unit,
        New(u8),
        Tuple(u8, u8),
        Struct { a: u8 },
    }
    let message = from_value::<Only>(&"two".into()).unwrap_err().to_string();
    assert_eq!(message, "unknown variant \"two\", expected \"One\"");
    assert_eq!(
        from_value::<E>(&ConfigValue::map([("Unit", ConfigValue::null())])).unwrap(),
        E::Unit
    );
    let unit_with_value = from_value::<E>(&ConfigValue::map([("Unit", 1.into())])).unwrap_err();
    assert_eq!(
        unit_with_value.to_string(),
        "variant \"Unit\" takes no value, found integer"
    );
    for variant in ["New", "Tuple", "Struct"] {
        let error = from_value::<E>(&variant.into()).unwrap_err().to_string();
        assert!(error.contains("write it as a map with one key"), "{error}");
    }
    let nested = from_value::<E>(&ConfigValue::map([(
        "Struct",
        ConfigValue::map([("a", "x".into())]),
    )]))
    .unwrap_err();
    assert_eq!(nested.path().unwrap().to_string(), "Struct.a");
}

#[test]
fn map_keys_can_be_newtypes_enums_and_options() {
    #[derive(Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct Name(String);
    #[derive(Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
    #[serde(rename_all = "lowercase")]
    enum Env {
        Dev,
        Prod,
    }
    let tree = ConfigValue::map([("dev", 1.into()), ("prod", 2.into())]);
    let by_name: BTreeMap<Name, u8> = from_value(&tree).unwrap();
    assert_eq!(by_name.get(&Name("prod".into())), Some(&2));
    let by_env: BTreeMap<Env, u8> = from_value(&tree).unwrap();
    assert_eq!(by_env.get(&Env::Dev), Some(&1));
    let by_opt: BTreeMap<Option<String>, u8> = from_value(&tree).unwrap();
    assert_eq!(by_opt.len(), 2);
    let typed: BTreeMap<i64, f32> = from_value(&ConfigValue::map([("-1", "0.5".into())])).unwrap();
    assert_eq!(typed.get(&-1), Some(&0.5));
    let chars: BTreeMap<char, u8> = from_value(&ConfigValue::map([("x", 1.into())])).unwrap();
    assert_eq!(chars.get(&'x'), Some(&1));
}

#[test]
fn tuple_structs_and_byte_bufs() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Pair(u8, u8);
    assert_eq!(
        from_value::<Pair>(&ConfigValue::array([1.into(), 2.into()])).unwrap(),
        Pair(1, 2)
    );
    assert_eq!(from_value::<Pair>(&"[3, 4]".into()).unwrap(), Pair(3, 4));

    struct BufVisitor;
    impl<'de> Visitor<'de> for BufVisitor {
        type Value = Vec<u8>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("bytes")
        }
        fn visit_bytes<E>(self, v: &[u8]) -> Result<Vec<u8>, E> {
            Ok(v.to_vec())
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
            let mut out = Vec::new();
            while let Some(byte) = seq.next_element()? {
                out.push(byte);
            }
            Ok(out)
        }
    }
    let value = ConfigValue::from("hi");
    assert_eq!((&value).deserialize_byte_buf(BufVisitor).unwrap(), b"hi");
    let array = ConfigValue::array([1.into(), 2.into()]);
    assert_eq!((&array).deserialize_bytes(BufVisitor).unwrap(), [1, 2]);
    assert!(
        (&ConfigValue::from(1))
            .deserialize_bytes(BufVisitor)
            .is_err()
    );
}

#[test]
fn identifiers_and_into_deserializer() {
    struct Ident;
    impl Visitor<'_> for Ident {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an identifier")
        }
        fn visit_str<E>(self, v: &str) -> Result<String, E> {
            Ok(v.to_owned())
        }
    }
    let value = ConfigValue::from("field");
    assert_eq!((&value).deserialize_identifier(Ident).unwrap(), "field");
    let de = (&value).into_deserializer();
    assert_eq!(String::deserialize(de).unwrap(), "field");
}

#[test]
fn deserialize_error_outside_a_tree() {
    let custom = <DeserializeError as de::Error>::custom("boom");
    assert_eq!(custom.to_string(), "boom");
    assert_eq!(custom.clone().into_config_error().to_string(), "boom");
    let missing = <DeserializeError as de::Error>::missing_field("port");
    assert_eq!(missing.to_string(), "missing field `port`");
    assert_eq!(
        cgp_config_core::ConfigError::from(missing).to_string(),
        "missing port"
    );
    let unknown = <DeserializeError as de::Error>::unknown_field("prot", &["port"]);
    assert_eq!(unknown.to_string(), "unknown field `prot`");
    assert_eq!(
        unknown.into_config_error().to_string(),
        "unknown key prot; did you mean \"port\"?"
    );
    // Errors from a tree display like the `ConfigError` they carry.
    let located = u8::deserialize(&ConfigValue::from("x")).unwrap_err();
    assert_eq!(located.to_string(), "expected u8, found string \"x\"");
}

#[test]
fn secret_json_objects_inside_arrays_stay_secret() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Cred {
        token: u32,
    }
    let text = ConfigValue::from(r#"[{"token": "s3cret"}]"#)
        .with_meta(Meta::new(Origin::env("CREDS")).with_secret(true));
    let error = from_value::<Vec<Cred>>(&text).unwrap_err();
    assert_eq!(
        error.to_string(),
        "[0].token: invalid secret value at env CREDS"
    );
}

#[test]
fn float_targets_accept_any_number() {
    assert_eq!(from_value::<f32>(&ConfigValue::from(-3)).unwrap(), -3.0);
    assert_eq!(
        from_value::<f64>(&ConfigValue::from(u64::MAX)).unwrap(),
        u64::MAX as f64
    );
    assert_eq!(
        from_value::<i128>(&ConfigValue::from(i64::MIN)).unwrap(),
        i128::from(i64::MIN)
    );
    let any: serde_json::Value = from_value(&ConfigValue::from(-1.5)).unwrap();
    assert_eq!(any, serde_json::json!(-1.5));
}

#[test]
fn value_debug_and_conversions() {
    let tree = ConfigValue::array([ConfigValue::null(), true.into(), 1.into(), "s".into()]);
    assert_eq!(format!("{tree:?}"), r#"[null, true, 1, "s"]"#);
    assert_eq!(ConfigValue::from(Arc::<str>::from("a")).as_str(), Some("a"));
    assert!(ConfigValue::from(None::<u8>).is_null());
    assert_eq!(ConfigValue::from(Some(2_u8)).as_u64(), Some(2));
    assert!(!ConfigValue::from(1).eq_with_meta(&ConfigValue::from("1")));
}

// --- JSON sequence text under the load limits --------------------------------------------

mod json_limits {
    use cgp_config_core::{ConfigError, LoadLimits};
    use cgp_config_value::{ConfigValue, from_value, from_value_with_limits};

    #[derive(serde::Deserialize, Debug)]
    struct Ids {
        ids: Vec<u32>,
    }

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct Two {
        a: Vec<u32>,
        b: Vec<u32>,
    }

    fn ids(text: &str) -> ConfigValue {
        ConfigValue::map([("ids", ConfigValue::from(text))])
    }

    fn json_array(len: usize) -> String {
        let items: Vec<String> = (0..len).map(|i| i.to_string()).collect();
        format!("[{}]", items.join(","))
    }

    #[test]
    #[cfg_attr(miri, ignore = "builds 100k+ nodes; far too slow under Miri")]
    fn a_wide_array_fails_with_max_keys_instead_of_allocating() {
        // The tree has one entry (`ids`), so four elements push it to five entries.
        let limits = LoadLimits::DEFAULT.with_max_keys(4);
        let error = from_value_with_limits::<Ids>(&ids("[1,2,3,4]"), &limits).unwrap_err();
        assert_eq!(error, ConfigError::limit_exceeded("max_keys", 4));
        // Exactly at the limit is fine.
        let (ok, _) = from_value_with_limits::<Ids>(&ids("[1,2,3]"), &limits).unwrap();
        assert_eq!(ok.ids, [1, 2, 3]);
        // The defaults apply to `from_value` (150k elements > 100k default keys).
        let error = from_value::<Ids>(&ids(&json_array(150_000))).unwrap_err();
        assert_eq!(error.code(), "config::limit_exceeded");
    }

    #[test]
    fn nesting_fails_with_max_depth_counted_from_the_string() {
        #[derive(serde::Deserialize, Debug)]
        #[allow(dead_code)]
        struct Nested {
            ids: Vec<Vec<Vec<u32>>>,
        }
        // `ids` sits at depth 1, so `[[[1]]]` puts the integer at depth 4.
        let limits = LoadLimits::DEFAULT.with_max_depth(3);
        let error = from_value_with_limits::<Nested>(&ids("[[[1]]]"), &limits).unwrap_err();
        assert_eq!(error, ConfigError::limit_exceeded("max_depth", 3));
        let limits = LoadLimits::DEFAULT.with_max_depth(4);
        assert!(from_value_with_limits::<Nested>(&ids("[[[1]]]"), &limits).is_ok());
    }

    #[test]
    fn max_depth_is_enforced_below_serde_jsons_own_recursion_limit() {
        // 100 levels is within serde_json's 128 but beyond the default `max_depth` of 64:
        // the error is the load limit, not a JSON syntax error.
        let text = format!("{}{}", "[".repeat(100), "]".repeat(100));
        let error = from_value::<serde_json::Value>(&ConfigValue::from(text.as_str()));
        // `serde_json::Value` asks for "any", which does not parse strings: use a sequence.
        assert!(error.is_ok());
        let error =
            from_value::<Vec<serde_json::Value>>(&ConfigValue::from(text.as_str())).unwrap_err();
        assert_eq!(error, ConfigError::limit_exceeded("max_depth", 64));
    }

    #[test]
    fn several_json_strings_share_one_key_budget() {
        let tree = ConfigValue::map([
            ("a", ConfigValue::from("[1,2]")),
            ("b", ConfigValue::from("[3,4]")),
        ]);
        // Two existing entries plus four elements.
        let limits = LoadLimits::DEFAULT.with_max_keys(5);
        let error = from_value_with_limits::<Two>(&tree, &limits).unwrap_err();
        assert_eq!(error.code(), "config::limit_exceeded");
        let limits = LoadLimits::DEFAULT.with_max_keys(6);
        assert!(from_value_with_limits::<Two>(&tree, &limits).is_ok());
    }

    #[test]
    fn empty_arrays_and_syntax_errors_are_unaffected() {
        let limits = LoadLimits::DEFAULT.with_max_keys(1).with_max_depth(1);
        let (empty, _) = from_value_with_limits::<Ids>(&ids("[]"), &limits).unwrap();
        assert!(empty.ids.is_empty());
        let error = from_value::<Ids>(&ids("[1,")).unwrap_err();
        assert_eq!(error.code(), "config::type");
        assert!(
            error.to_string().contains("invalid JSON array text"),
            "{error}"
        );
        let error = from_value::<Ids>(&ids("[1] x")).unwrap_err();
        assert_eq!(error.code(), "config::type");
    }

    #[test]
    #[cfg_attr(miri, ignore = "builds 100k+ nodes; far too slow under Miri")]
    fn the_standalone_deserializer_applies_the_defaults() {
        use serde::Deserialize;
        let text = ConfigValue::from(json_array(150_000).as_str());
        let error = Vec::<u32>::deserialize(&text).unwrap_err();
        assert_eq!(error.into_config_error().code(), "config::limit_exceeded");
        let duplicate = ConfigValue::from(r#"[{"a":1,"a":2}]"#);
        assert!(Vec::<serde_json::Value>::deserialize(&duplicate).is_err());
    }
}

// --- Floats stay finite; keys follow value rules -----------------------------------------

#[test]
fn float_coercion_rejects_overflow_to_infinity() {
    // A finite f64 that does not fit an f32.
    let error = from_value::<f32>(&ConfigValue::from(1e300)).unwrap_err();
    assert_eq!(error.code(), "config::type");
    assert!(error.to_string().contains("out of range"), "{error}");
    // Overflowing text.
    assert_eq!(code::<f32>(&ConfigValue::from("1e39")), "config::type");
    assert_eq!(code::<f64>(&ConfigValue::from("1e400")), "config::type");
    // The widest integers fit (u64::MAX is about 1.8e19, far below f32::MAX).
    let wide: f32 = from_value(&ConfigValue::from(u64::MAX)).unwrap();
    assert!(wide.is_finite());
    let wide: f64 = from_value(&ConfigValue::from(i64::MIN)).unwrap();
    assert!(wide.is_finite());
    // Explicit non-finite values pass through.
    assert!(
        from_value::<f32>(&ConfigValue::from("-inf"))
            .unwrap()
            .is_infinite()
    );
    assert!(
        from_value::<f64>(&ConfigValue::from("NaN"))
            .unwrap()
            .is_nan()
    );
    assert!(
        from_value::<f32>(&ConfigValue::from(f64::INFINITY))
            .unwrap()
            .is_infinite()
    );
}

#[test]
fn map_keys_parse_bools_and_floats_like_values() {
    use std::collections::HashMap;
    let tree = ConfigValue::map([("TRUE", 1.into()), ("False", 2.into())]);
    let by_bool: HashMap<bool, u8> = from_value(&tree).unwrap();
    assert_eq!(by_bool.get(&true), Some(&1));
    assert_eq!(by_bool.get(&false), Some(&2));
    assert_eq!(
        code::<HashMap<bool, u8>>(&ConfigValue::map([("yes", 1.into())])),
        "config::type"
    );

    /// An `f32` map key (ordered by bits, since `f32` is not `Ord`).
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct F32Key(u32);
    impl<'de> Deserialize<'de> for F32Key {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            f32::deserialize(deserializer).map(|value| Self(value.to_bits()))
        }
    }
    let floats: BTreeMap<F32Key, u8> =
        from_value(&ConfigValue::map([("1.5", 1.into()), ("-inf", 2.into())])).unwrap();
    assert!(floats.contains_key(&F32Key(1.5_f32.to_bits())));
    assert!(floats.contains_key(&F32Key(f32::NEG_INFINITY.to_bits())));
    // Overflowing key text is rejected like overflowing value text.
    assert_eq!(
        code::<BTreeMap<F32Key, u8>>(&ConfigValue::map([("1e39", 1.into())])),
        "config::type"
    );
    let error = from_value::<HashMap<u32, u8>>(&ConfigValue::map([("x", 1.into())])).unwrap_err();
    assert_eq!(error.code(), "config::type");
}

// --- `deserialize_any` does not coerce strings (a deliberate choice) --------------------

#[test]
fn deserialize_any_sees_env_strings_as_strings() {
    #[derive(Deserialize, Debug, PartialEq)]
    #[serde(untagged)]
    enum Port {
        Number(u16),
        Named(String),
    }
    #[derive(Deserialize, Debug, PartialEq)]
    struct Inner {
        port: u16,
    }
    #[derive(Deserialize, Debug)]
    struct Flattened {
        #[serde(flatten)]
        rest: std::collections::HashMap<String, serde_json::Value>,
    }

    let env = ConfigValue::from("8080");
    // Untagged enums ask for "any": the env string stays a string.
    assert_eq!(
        from_value::<Port>(&env).unwrap(),
        Port::Named("8080".into())
    );
    // A typed file value still picks the number variant.
    assert_eq!(
        from_value::<Port>(&ConfigValue::from(8080)).unwrap(),
        Port::Number(8080)
    );
    // `serde_json::Value` and flattened maps see the raw string too.
    assert_eq!(
        from_value::<serde_json::Value>(&env).unwrap(),
        serde_json::json!("8080")
    );
    let flat: Flattened = from_value(&ConfigValue::map([("port", env.clone())])).unwrap();
    assert_eq!(flat.rest["port"], serde_json::json!("8080"));
    // A concrete type coerces, which is the recommended fix.
    assert_eq!(
        from_value::<Inner>(&ConfigValue::map([("port", env)])).unwrap(),
        Inner { port: 8080 }
    );
}
