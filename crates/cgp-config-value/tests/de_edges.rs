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
