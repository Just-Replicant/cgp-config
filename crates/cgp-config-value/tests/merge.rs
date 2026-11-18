//! Merge semantics: every policy, conflicts, provenance, and structural sharing.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

use cgp_config_core::{ConfigError, KeyPath, LoadLimits, Origin};
use cgp_config_value::merge::{ArrayMerge, MergeDepth, MergeOptions, Precedence, merge};
use cgp_config_value::{ConfigValue, Kind};

fn at(value: &ConfigValue, path: &str) -> ConfigValue {
    value
        .get_path(&KeyPath::parse(path).unwrap())
        .unwrap_or_else(|| panic!("no value at {path}"))
        .clone()
}

fn layer(origin: Origin, value: ConfigValue) -> ConfigValue {
    let mut value = value;
    value.stamp_origin(&origin);
    value
}

fn deep(base: ConfigValue, overlay: ConfigValue) -> Result<ConfigValue, ConfigError> {
    merge(base, overlay, &MergeOptions::DEEP)
}

#[test]
fn maps_merge_recursively_and_keep_base_order() {
    let base = ConfigValue::map([
        ("a", 1.into()),
        ("m", ConfigValue::map([("x", 1.into()), ("y", 2.into())])),
    ]);
    let overlay = ConfigValue::map([
        ("m", ConfigValue::map([("y", 20.into()), ("z", 30.into())])),
        ("b", 2.into()),
    ]);
    let merged = deep(base, overlay).unwrap();
    let keys: Vec<_> = merged.as_map().unwrap().keys().collect();
    assert_eq!(keys, ["a", "m", "b"]);
    let inner: Vec<_> = at(&merged, "m")
        .as_map()
        .unwrap()
        .keys()
        .map(str::to_owned)
        .collect();
    assert_eq!(inner, ["x", "y", "z"]);
    assert_eq!(at(&merged, "m.y").as_u64(), Some(20));
}

#[test]
fn scalars_and_nulls_replace() {
    let merged = deep(
        ConfigValue::map([
            ("a", 1.into()),
            ("b", ConfigValue::null()),
            ("c", "s".into()),
        ]),
        ConfigValue::map([
            ("a", true.into()),
            ("b", 2.into()),
            ("c", ConfigValue::null()),
        ]),
    )
    .unwrap();
    assert_eq!(at(&merged, "a").as_bool(), Some(true));
    assert_eq!(at(&merged, "b").as_u64(), Some(2));
    assert!(at(&merged, "c").is_null());

    // A null replaces even a map: explicit "unset".
    let merged = deep(
        ConfigValue::map([("m", ConfigValue::map([("x", 1.into())]))]),
        ConfigValue::map([("m", ConfigValue::null())]),
    )
    .unwrap();
    assert!(at(&merged, "m").is_null());
}

#[test]
fn arrays_replace_by_default() {
    let merged = deep(
        ConfigValue::map([("v", ConfigValue::array([1.into(), 2.into()]))]),
        ConfigValue::map([("v", ConfigValue::array([3.into()]))]),
    )
    .unwrap();
    assert_eq!(at(&merged, "v").as_array().unwrap().len(), 1);
}

#[test]
fn a_string_may_replace_an_array() {
    // Env layers carry arrays as JSON text; the deserializer parses them later.
    let merged = deep(
        ConfigValue::map([("v", ConfigValue::array([1.into()]))]),
        ConfigValue::map([("v", "[1, 2]".into())]),
    )
    .unwrap();
    assert_eq!(at(&merged, "v").as_str(), Some("[1, 2]"));
}

#[test]
fn shape_changes_conflict_with_both_origins() {
    let base = layer(
        Origin::file("base.toml"),
        ConfigValue::map([("db", ConfigValue::map([("url", "x".into())]))]),
    );
    let overlay = layer(Origin::env("APP__DB"), ConfigValue::map([("db", 5.into())]));
    let error = deep(base, overlay).unwrap_err();
    assert_eq!(error.code(), "config::merge_conflict");
    assert_eq!(error.path().unwrap().to_string(), "db");
    assert_eq!(
        error.to_string(),
        "cannot merge db: map from base.toml vs integer from env APP__DB"
    );

    for (base, overlay) in [
        (ConfigValue::array([]), ConfigValue::map([("a", 1.into())])),
        (ConfigValue::map([("a", 1.into())]), ConfigValue::array([])),
        (ConfigValue::from(1), ConfigValue::array([])),
        (ConfigValue::array([]), ConfigValue::from(1)),
        (ConfigValue::from("s"), ConfigValue::map([("a", 1.into())])),
    ] {
        let wrapped = deep(
            ConfigValue::map([("k", base)]),
            ConfigValue::map([("k", overlay)]),
        );
        assert_eq!(wrapped.unwrap_err().code(), "config::merge_conflict");
    }
}

#[test]
fn conflicts_report_nested_and_indexed_paths() {
    let options = MergeOptions::DEEP.with_arrays(ArrayMerge::ByKey("name"));
    let base = ConfigValue::map([(
        "servers",
        ConfigValue::array([ConfigValue::map([
            ("name", "a".into()),
            ("tls", ConfigValue::empty_map()),
        ])]),
    )]);
    let overlay = ConfigValue::map([(
        "servers",
        ConfigValue::array([ConfigValue::map([
            ("name", "a".into()),
            ("tls", true.into()),
        ])]),
    )]);
    let error = merge(base, overlay, &options).unwrap_err();
    assert_eq!(error.path().unwrap().to_string(), "servers[0].tls");
}

#[test]
fn winner_records_the_shadowed_layer() {
    let base = layer(
        Origin::file("a.toml"),
        ConfigValue::map([("port", 1.into())]),
    );
    let middle = layer(
        Origin::file("b.toml"),
        ConfigValue::map([("port", 2.into())]),
    );
    let top = layer(Origin::env("PORT"), ConfigValue::map([("port", 3.into())]));
    let merged = deep(deep(base, middle).unwrap(), top).unwrap();
    let port = at(&merged, "port");
    assert_eq!(port.meta().origin, Origin::env("PORT"));
    let chain: Vec<_> = port
        .meta()
        .shadowed
        .iter()
        .map(|p| p.origin.to_string())
        .collect();
    assert_eq!(chain, ["b.toml", "a.toml"]);
    assert!(!port.meta().truncated);
}

#[test]
fn provenance_chain_is_capped() {
    let options = MergeOptions::DEEP.with_max_provenance(2);
    let mut acc = layer(Origin::file("0"), ConfigValue::map([("k", 0.into())]));
    for i in 1..6 {
        let next = layer(
            Origin::file(i.to_string()),
            ConfigValue::map([("k", i.into())]),
        );
        acc = merge(acc, next, &options).unwrap();
    }
    let k = at(&acc, "k");
    assert_eq!(k.meta().shadowed.len(), 2);
    assert!(k.meta().truncated);
    let chain: Vec<_> = k
        .meta()
        .shadowed
        .iter()
        .map(|p| p.origin.to_string())
        .collect();
    assert_eq!(chain, ["4", "3"]);
}

#[test]
fn untouched_subtrees_are_shared_not_copied() {
    let big = ConfigValue::map([("x", ConfigValue::array((0..100).map(ConfigValue::from)))]);
    let base = ConfigValue::map([("untouched", big.clone()), ("port", 1.into())]);
    let overlay_only = ConfigValue::map([("added", ConfigValue::map([("y", 1.into())]))]);
    let overlay = ConfigValue::map([
        ("port", 2.into()),
        ("added", overlay_only.get("added").unwrap().clone()),
    ]);
    let merged = deep(base, overlay).unwrap();
    assert!(ConfigValue::ptr_eq(merged.get("untouched").unwrap(), &big));
    assert!(ConfigValue::ptr_eq(
        merged.get("added").unwrap(),
        overlay_only.get("added").unwrap()
    ));
}

#[test]
fn base_wins_keeps_the_lower_layer() {
    let options = MergeOptions::DEEP.with_precedence(Precedence::BaseWins);
    let merged = merge(
        layer(
            Origin::Defaults,
            ConfigValue::map([("a", 1.into()), ("m", ConfigValue::map([("x", 1.into())]))]),
        ),
        layer(
            Origin::file("f"),
            ConfigValue::map([
                ("a", 2.into()),
                ("b", 3.into()),
                ("m", ConfigValue::map([("x", 2.into()), ("y", 2.into())])),
            ]),
        ),
        &options,
    )
    .unwrap();
    assert_eq!(at(&merged, "a").as_u64(), Some(1));
    assert_eq!(at(&merged, "b").as_u64(), Some(3));
    assert_eq!(at(&merged, "m.x").as_u64(), Some(1));
    assert_eq!(at(&merged, "m.y").as_u64(), Some(2));
    assert_eq!(
        at(&merged, "a").meta().shadowed[0].origin,
        Origin::file("f")
    );
}

#[test]
fn append_concatenates_arrays() {
    for (precedence, origin) in [
        (Precedence::OverlayWins, Origin::file("top")),
        (Precedence::BaseWins, Origin::file("bottom")),
    ] {
        let options = MergeOptions::DEEP
            .with_arrays(ArrayMerge::Append)
            .with_precedence(precedence);
        let merged = merge(
            layer(
                Origin::file("bottom"),
                ConfigValue::map([("v", ConfigValue::array([1.into()]))]),
            ),
            layer(
                Origin::file("top"),
                ConfigValue::map([("v", ConfigValue::array([2.into(), 3.into()]))]),
            ),
            &options,
        )
        .unwrap();
        let v = at(&merged, "v");
        let items: Vec<_> = v
            .as_array()
            .unwrap()
            .iter()
            .filter_map(ConfigValue::as_u64)
            .collect();
        assert_eq!(items, [1, 2, 3]);
        assert_eq!(v.meta().origin, origin);
        assert_eq!(v.meta().shadowed.len(), 1);
    }
}

#[test]
fn by_key_merges_matching_elements_and_appends_new_ones() {
    let options = MergeOptions::DEEP.with_arrays(ArrayMerge::ByKey("name"));
    let base = ConfigValue::map([(
        "servers",
        ConfigValue::array([
            ConfigValue::map([
                ("name", "a".into()),
                ("port", 1.into()),
                ("tls", false.into()),
            ]),
            ConfigValue::map([("name", "b".into()), ("port", 2.into())]),
            ConfigValue::map([("port", 9.into())]),
        ]),
    )]);
    let overlay = ConfigValue::map([(
        "servers",
        ConfigValue::array([
            ConfigValue::map([("name", "b".into()), ("port", 20.into())]),
            ConfigValue::map([("name", "c".into()), ("port", 3.into())]),
            ConfigValue::map([("port", 10.into())]),
            ConfigValue::from("scalar element"),
        ]),
    )]);
    let merged = merge(base, overlay, &options).unwrap();
    let servers = at(&merged, "servers");
    let servers = servers.as_array().unwrap();
    assert_eq!(servers.len(), 6);
    assert_eq!(
        servers[1].get("port").and_then(ConfigValue::as_u64),
        Some(20)
    );
    assert_eq!(
        servers[3].get("name").and_then(ConfigValue::as_str),
        Some("c")
    );
    // Elements without a key never match.
    assert_eq!(
        servers[2].get("port").and_then(ConfigValue::as_u64),
        Some(9)
    );
    assert_eq!(
        servers[4].get("port").and_then(ConfigValue::as_u64),
        Some(10)
    );
}

#[test]
fn by_key_rejects_duplicate_keys_on_either_side() {
    let options = MergeOptions::DEEP.with_arrays(ArrayMerge::ByKey("name"));
    let dup = ConfigValue::array([
        ConfigValue::map([("name", "a".into())]),
        ConfigValue::map([("name", "a".into())]),
    ]);
    let ok = ConfigValue::array([]);
    for (base, overlay) in [(dup.clone(), ok.clone()), (ok, dup)] {
        let error = merge(
            ConfigValue::map([("s", base)]),
            ConfigValue::map([("s", overlay)]),
            &options,
        )
        .unwrap_err();
        assert_eq!(error.code(), "config::duplicate_key");
        assert_eq!(error.path().unwrap().to_string(), "s[1].name");
    }
}

#[test]
fn shallow_replaces_top_level_entries_whole() {
    let options = MergeOptions::DEEP.with_depth(MergeDepth::Shallow);
    let merged = merge(
        ConfigValue::map([
            ("m", ConfigValue::map([("x", 1.into()), ("y", 1.into())])),
            ("k", 1.into()),
        ]),
        ConfigValue::map([
            ("m", ConfigValue::map([("x", 2.into())])),
            ("n", ConfigValue::array([])),
        ]),
        &options,
    )
    .unwrap();
    assert!(at(&merged, "m").get("y").is_none());
    assert_eq!(at(&merged, "k").as_u64(), Some(1));
    // Shape changes below the top level are not conflicts in shallow mode.
    let merged = merge(
        ConfigValue::map([("m", ConfigValue::empty_map())]),
        ConfigValue::map([("m", 1.into())]),
        &options,
    )
    .unwrap();
    assert_eq!(at(&merged, "m").as_u64(), Some(1));
}

#[test]
fn top_level_scalars_merge_too() {
    let merged = deep(ConfigValue::from(1), ConfigValue::from(2)).unwrap();
    assert_eq!(merged.as_u64(), Some(2));
    assert!(matches!(
        deep(ConfigValue::null(), ConfigValue::array([]))
            .unwrap()
            .kind(),
        Kind::Array(_)
    ));
}

#[test]
fn default_options_are_deep() {
    assert_eq!(MergeOptions::default(), MergeOptions::DEEP);
    assert_eq!(
        MergeOptions::DEEP.max_provenance,
        LoadLimits::DEFAULT.max_provenance
    );
}
