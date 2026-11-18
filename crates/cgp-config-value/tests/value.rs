//! `ConfigValue` accessors, metadata, redaction, limits, and serde round trips.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

use cgp_config_core::{KeyPath, LoadLimits, Meta, Origin};
use cgp_config_value::{ConfigMap, ConfigValue, Kind, Number, REDACTED};

fn sample() -> ConfigValue {
    ConfigValue::map([
        ("name", "demo".into()),
        ("on", true.into()),
        ("n", 3.into()),
        ("neg", (-3).into()),
        ("f", 0.5.into()),
        (
            "list",
            ConfigValue::array(["a".into(), ConfigValue::map([("x", 1.into())])]),
        ),
        ("nothing", ConfigValue::null()),
    ])
}

#[test]
fn accessors_return_the_right_kinds() {
    let v = sample();
    assert_eq!(v.get("name").and_then(ConfigValue::as_str), Some("demo"));
    assert_eq!(v.get("on").and_then(ConfigValue::as_bool), Some(true));
    assert_eq!(v.get("n").and_then(ConfigValue::as_u64), Some(3));
    assert_eq!(v.get("neg").and_then(ConfigValue::as_i64), Some(-3));
    assert_eq!(v.get("neg").and_then(ConfigValue::as_u64), None);
    assert_eq!(v.get("f").and_then(ConfigValue::as_f64), Some(0.5));
    assert_eq!(
        v.get("n").and_then(ConfigValue::as_number),
        Some(&Number::from_u64(3))
    );
    assert!(v.get("nothing").unwrap().is_null());
    assert!(v.is_map() && !v.is_array());
    assert!(v.get("list").unwrap().is_array());
    assert_eq!(
        v.get("list")
            .unwrap()
            .get_index(0)
            .and_then(ConfigValue::as_str),
        Some("a")
    );
    assert!(v.get("name").unwrap().get("x").is_none());
    assert!(v.get("name").unwrap().get_index(0).is_none());
    assert_eq!(v.get("name").and_then(ConfigValue::as_bool), None);
    assert_eq!(v.get("on").and_then(ConfigValue::as_str), None);
    assert!(v.get("on").unwrap().as_array().is_none());
    assert!(v.get("on").unwrap().as_map().is_none());
    assert!(v.get("on").unwrap().as_number().is_none());
    let kinds: Vec<_> = v
        .as_map()
        .unwrap()
        .values()
        .map(ConfigValue::kind_name)
        .collect();
    assert_eq!(
        kinds,
        [
            "string", "bool", "integer", "integer", "float", "array", "null"
        ]
    );
}

#[test]
fn paths_walk_maps_and_arrays() {
    let v = sample();
    let x = v.get_path(&KeyPath::parse("list[1].x").unwrap());
    assert_eq!(x.and_then(ConfigValue::as_u64), Some(1));
    assert!(v.get_path(&KeyPath::parse("list[5]").unwrap()).is_none());
    assert!(
        v.get_path(&KeyPath::parse("name.deeper").unwrap())
            .is_none()
    );
    assert!(ConfigValue::ptr_eq(
        v.get_path(&KeyPath::root()).unwrap(),
        &v
    ));
}

#[test]
fn equality_ignores_metadata_unless_asked() {
    let a = ConfigValue::from(1).with_origin(Origin::file("a"));
    let b = ConfigValue::from(1).with_origin(Origin::file("b"));
    assert_eq!(a, b);
    assert!(!a.eq_with_meta(&b));
    assert!(a.eq_with_meta(&a.clone()));
    let tree_a = ConfigValue::map([("k", ConfigValue::array([a.clone()]))]);
    let tree_b = ConfigValue::map([("k", ConfigValue::array([b]))]);
    assert_eq!(tree_a, tree_b);
    assert!(!tree_a.eq_with_meta(&tree_b));
    let tree_c = ConfigValue::map([("k", ConfigValue::array([a]))]);
    assert!(tree_a.eq_with_meta(&tree_c));
    assert_ne!(ConfigValue::from(1), ConfigValue::from(1.0));
    assert_ne!(ConfigValue::from("1"), ConfigValue::from(1));
}

#[test]
fn stamping_sets_every_node() {
    let mut v = ConfigValue::map([
        ("a", 1.into()),
        ("b", ConfigValue::from(2).with_origin(Origin::cli("--b"))),
        ("c", ConfigValue::array([3.into()])),
    ]);
    v.stamp_origin(&Origin::file("f"));
    assert_eq!(v.meta().origin, Origin::file("f"));
    assert_eq!(v.get("a").unwrap().meta().origin, Origin::file("f"));
    assert_eq!(v.get("b").unwrap().meta().origin, Origin::file("f"));
    assert_eq!(
        v.get("c").unwrap().get_index(0).unwrap().meta().origin,
        Origin::file("f")
    );
}

#[test]
fn copy_on_write_keeps_clones_independent() {
    let original = sample();
    let mut copy = original.clone();
    assert!(ConfigValue::ptr_eq(&original, &copy));
    copy.as_map_mut().unwrap().insert("added", 1.into());
    copy.meta_mut().secret = true;
    assert!(!ConfigValue::ptr_eq(&original, &copy));
    assert!(original.get("added").is_none());
    assert!(!original.meta().secret);
    *copy.kind_mut() = Kind::Bool(false);
    assert_eq!(copy.as_bool(), Some(false));
    let (kind, meta) = copy.into_parts();
    assert!(matches!(kind, Kind::Bool(false)));
    assert!(meta.secret);
    assert!(ConfigValue::from(1).as_map_mut().is_none());
}

#[test]
fn size_and_limits() {
    let v = sample();
    assert_eq!(v.count_entries(), 7 + 2 + 1);
    assert_eq!(v.depth(), 3);
    assert_eq!(ConfigValue::from(1).depth(), 0);
    assert!(v.check_limits(&LoadLimits::DEFAULT).is_ok());
    assert_eq!(
        v.check_limits(&LoadLimits::DEFAULT.with_max_keys(5))
            .unwrap_err()
            .to_string(),
        "limit exceeded: max_keys (5)"
    );
    assert_eq!(
        v.check_limits(&LoadLimits::DEFAULT.with_max_depth(2))
            .unwrap_err()
            .code(),
        "config::limit_exceeded"
    );
    assert!(v.check_limits(&LoadLimits::UNLIMITED).is_ok());
}

#[test]
fn secrets_are_redacted_in_debug_and_dumps() {
    let secret =
        ConfigValue::from("hunter2").with_meta(Meta::new(Origin::env("PW")).with_secret(true));
    let tree = ConfigValue::map([("pw", secret), ("user", "bob".into())]);
    let debug = format!("{tree:?}");
    assert!(!debug.contains("hunter2"), "{debug}");
    assert!(debug.contains("bob"));
    let redacted = tree.redacted();
    assert_eq!(
        redacted.get("pw").and_then(ConfigValue::as_str),
        Some(REDACTED)
    );
    assert_eq!(
        redacted.get("user").and_then(ConfigValue::as_str),
        Some("bob")
    );
    let json = serde_json::to_string(&redacted).unwrap();
    assert_eq!(json, r#"{"pw":"[REDACTED]","user":"bob"}"#);
    let arr =
        ConfigValue::array([ConfigValue::from(1).with_meta(Meta::default().with_secret(true))])
            .redacted();
    assert_eq!(
        arr.get_index(0).and_then(ConfigValue::as_str),
        Some(REDACTED)
    );
}

#[test]
fn conversions_and_defaults() {
    assert!(
        ConfigValue::default()
            .as_map()
            .is_some_and(ConfigMap::is_empty)
    );
    assert_eq!(ConfigValue::from(String::from("s")).as_str(), Some("s"));
    assert_eq!(ConfigValue::from(7_u8).as_u64(), Some(7));
    assert_eq!(ConfigValue::from(-7_i32).as_i64(), Some(-7));
    assert_eq!(ConfigValue::from(1.5_f32).as_f64(), Some(1.5));
    let from_vec = ConfigValue::from(vec![ConfigValue::from(1)]);
    assert!(from_vec.is_array());
    let map: ConfigMap = [("a", ConfigValue::from(1))].into_iter().collect();
    assert!(ConfigValue::from(map).is_map());
    assert!(ConfigValue::from(Kind::Null).is_null());
}

#[test]
fn json_round_trip_keeps_order_and_rejects_duplicates() {
    let text = r#"{"z":1,"a":[true,null,"s",-1,1.5],"m":{}}"#;
    let v: ConfigValue = serde_json::from_str(text).unwrap();
    assert_eq!(serde_json::to_string(&v).unwrap(), text);
    let error = serde_json::from_str::<ConfigValue>(r#"{"a":1,"a":2}"#).unwrap_err();
    assert!(error.to_string().contains("duplicate key"), "{error}");
}

#[test]
fn map_api() {
    let mut map = ConfigMap::with_capacity(2);
    assert!(map.is_empty());
    map.insert("b", 1.into());
    map.insert("a", 2.into());
    assert_eq!(map.insert("b", 3.into()).and_then(|v| v.as_u64()), Some(1));
    assert!(map.contains_key("a"));
    assert_eq!(map.keys().collect::<Vec<_>>(), ["b", "a"]);
    for value in map.values_mut() {
        *value = ConfigValue::from(0);
    }
    assert!(map.values().all(|v| v.as_u64() == Some(0)));
    assert_eq!(map.iter().next_back().map(|(k, _)| k), Some("a"));
    assert_eq!(map.remove("b").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(map.len(), 1);
    map.extend([("c", ConfigValue::null())]);
    let owned: Vec<_> = map
        .clone()
        .into_iter()
        .map(|(k, _)| k.to_string())
        .collect();
    assert_eq!(owned, ["a", "c"]);
    assert_eq!(
        map,
        map.iter_shared()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    );
    assert!(map.get_mut("missing").is_none());
}
