//! Property tests: the merge algebra, and env strings round-tripping through the deserializer.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

use cgp_config_value::merge::{MergeOptions, merge};
use cgp_config_value::{ConfigValue, from_value};
use proptest::prelude::*;

fn cases() -> ProptestConfig {
    // Miri runs a handful; native CI runs the default 256.
    ProptestConfig::with_cases(if cfg!(miri) { 4 } else { 256 })
}

fn scalar() -> impl Strategy<Value = ConfigValue> {
    prop_oneof![
        any::<bool>().prop_map(ConfigValue::from),
        any::<i64>().prop_map(ConfigValue::from),
        "[a-z]{0,4}".prop_map(ConfigValue::from),
    ]
}

/// Trees without nulls and with a fixed shape per key name, so merges never conflict:
/// keys starting with `m` hold maps, `a` arrays, others scalars.
fn tree(depth: u32) -> BoxedStrategy<ConfigValue> {
    let leaf_entries = prop::collection::vec(("[s-v]", scalar()), 0..4);
    if depth == 0 {
        return leaf_entries.prop_map(ConfigValue::map).boxed();
    }
    let maps = prop::collection::vec(("m[0-2]", tree(depth - 1)), 0..3);
    let arrays = prop::collection::vec(
        (
            "a[0-2]",
            prop::collection::vec(scalar(), 0..3).prop_map(ConfigValue::array),
        ),
        0..2,
    );
    (leaf_entries, maps, arrays)
        .prop_map(|(mut leaves, maps, arrays)| {
            leaves.extend(maps);
            leaves.extend(arrays);
            ConfigValue::map(leaves)
        })
        .boxed()
}

fn json(value: &ConfigValue) -> serde_json::Value {
    from_value(value).unwrap()
}

proptest! {
    #![proptest_config(cases())]

    #[test]
    fn deep_merge_is_associative(a in tree(2), b in tree(2), c in tree(2)) {
        let opts = MergeOptions::DEEP;
        let left = merge(merge(a.clone(), b.clone(), &opts).unwrap(), c.clone(), &opts).unwrap();
        let right = merge(a, merge(b, c, &opts).unwrap(), &opts).unwrap();
        prop_assert_eq!(&left, &right);
        // Key order is part of the result too.
        prop_assert_eq!(json(&left).to_string(), json(&right).to_string());
    }

    #[test]
    fn empty_map_is_the_identity(a in tree(2)) {
        let opts = MergeOptions::DEEP;
        prop_assert_eq!(&merge(ConfigValue::empty_map(), a.clone(), &opts).unwrap(), &a);
        prop_assert_eq!(&merge(a.clone(), ConfigValue::empty_map(), &opts).unwrap(), &a);
    }

    #[test]
    fn merging_with_itself_is_idempotent(a in tree(2)) {
        let opts = MergeOptions::DEEP;
        prop_assert_eq!(&merge(a.clone(), a.clone(), &opts).unwrap(), &a);
    }

    #[test]
    fn the_later_scalar_wins(key in "[a-z]{1,3}", x in scalar(), y in scalar()) {
        let merged = merge(
            ConfigValue::map([(key.as_str(), x)]),
            ConfigValue::map([(key.as_str(), y.clone())]),
            &MergeOptions::DEEP,
        ).unwrap();
        prop_assert_eq!(merged.get(&key).unwrap(), &y);
    }

    #[test]
    fn map_over_scalar_conflicts(key in "[a-z]{1,3}", x in scalar(), m in tree(1)) {
        let result = merge(
            ConfigValue::map([(key.as_str(), x)]),
            ConfigValue::map([(key.as_str(), m)]),
            &MergeOptions::DEEP,
        );
        prop_assert_eq!(result.unwrap_err().code(), "config::merge_conflict");
    }

    #[test]
    fn every_overlay_leaf_survives(a in tree(2), b in tree(2)) {
        let merged = merge(a, b.clone(), &MergeOptions::DEEP).unwrap();
        fn check(merged: &ConfigValue, overlay: &ConfigValue) -> bool {
            match overlay.as_map() {
                Some(map) => map.iter().all(|(k, v)| merged.get(k).is_some_and(|m| check(m, v))),
                None => merged == overlay,
            }
        }
        prop_assert!(check(&merged, &b));
    }
}

/// What an env layer holds after the source splits `APP__*` keys: strings only.
#[derive(serde::Deserialize, Debug, PartialEq, Clone)]
struct EnvShaped {
    port: u16,
    offset: i64,
    ratio: f64,
    enabled: bool,
    name: String,
    ids: Vec<u32>,
    level: Level,
    nested: Nested,
}

#[derive(serde::Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
enum Level {
    Debug,
    Info,
}

#[derive(serde::Deserialize, Debug, PartialEq, Clone)]
struct Nested {
    workers: Option<u8>,
}

fn env_shaped() -> impl Strategy<Value = EnvShaped> {
    (
        any::<u16>(),
        any::<i64>(),
        -1.0e6..1.0e6_f64,
        any::<bool>(),
        "[ -~]{0,8}",
        prop::collection::vec(any::<u32>(), 0..4),
        prop_oneof![Just(Level::Debug), Just(Level::Info)],
        any::<Option<u8>>(),
    )
        .prop_map(
            |(port, offset, ratio, enabled, name, ids, level, workers)| EnvShaped {
                port,
                offset,
                ratio,
                enabled,
                name,
                ids,
                level,
                nested: Nested { workers },
            },
        )
}

/// The env text a user would export for `config`.
fn to_env_tree(config: &EnvShaped) -> ConfigValue {
    let ids: Vec<String> = config.ids.iter().map(u32::to_string).collect();
    let level = match config.level {
        Level::Debug => "debug",
        Level::Info => "info",
    };
    let mut nested = Vec::new();
    if let Some(workers) = config.nested.workers {
        nested.push(("workers", ConfigValue::from(workers.to_string())));
    }
    ConfigValue::map([
        ("port", ConfigValue::from(config.port.to_string())),
        ("offset", ConfigValue::from(config.offset.to_string())),
        ("ratio", ConfigValue::from(config.ratio.to_string())),
        (
            "enabled",
            ConfigValue::from(config.enabled.to_string().to_uppercase()),
        ),
        ("name", ConfigValue::from(config.name.as_str())),
        ("ids", ConfigValue::from(format!("[{}]", ids.join(", ")))),
        ("level", ConfigValue::from(level)),
        ("nested", ConfigValue::map(nested)),
    ])
}

proptest! {
    #![proptest_config(cases())]

    /// struct → env text → tree of strings → struct is the identity.
    #[test]
    fn env_strings_round_trip_through_the_deserializer(config in env_shaped()) {
        let tree = to_env_tree(&config);
        let back: EnvShaped = from_value(&tree).unwrap();
        prop_assert_eq!(back, config);
    }
}
