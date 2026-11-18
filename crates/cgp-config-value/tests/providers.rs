//! Each merge provider and unknown-key policy behind the CGP interfaces.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

use cgp_config_core::components::*;
use cgp_config_core::{ConfigError, DefaultLoadLimits, IgnoreWarnings};
use cgp_config_value::{
    AdjoinArrays, Admerge, AppendArrays, ConfigValue, DeepMerge, IgnoreUnknownKeys, JoinBaseWins,
    MergeArraysByKey, SerdeWithProvenance, ShallowReplace,
};
use cgp_fork::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_fork::extra::error::RaiseFrom;
use cgp_fork::prelude::*;
use serde::Deserialize;

macro_rules! merger_context {
    ($name:ident, $merger:ty) => {
        struct $name;
        delegate_components! {
            #[unchecked]
            $name {
                ErrorTypeProviderComponent: UseType<ConfigError>,
                ErrorRaiserComponent: RaiseFrom,
                ConfigValueTypeProviderComponent: UseType<ConfigValue>,
                LoadLimitsComponent: DefaultLoadLimits,
                LoadReportComponent: IgnoreWarnings,
                LayerMergerComponent: $merger,
                ConfigDeserializerComponent: SerdeWithProvenance<IgnoreUnknownKeys>,
            }
        }
    };
}

merger_context!(Deep, DeepMerge);
merger_context!(Shallow, ShallowReplace);
merger_context!(Append, AppendArrays);
merger_context!(Admerged, Admerge);
merger_context!(Join, JoinBaseWins);
merger_context!(Adjoin, AdjoinArrays);
merger_context!(ByName, MergeArraysByKey<Symbol!("name")>);

fn base() -> ConfigValue {
    ConfigValue::map([
        ("v", ConfigValue::array([1.into()])),
        ("m", ConfigValue::map([("x", 1.into()), ("y", 1.into())])),
        ("s", 1.into()),
        (
            "named",
            ConfigValue::array([ConfigValue::map([("name", "a".into()), ("p", 1.into())])]),
        ),
    ])
}

fn overlay() -> ConfigValue {
    ConfigValue::map([
        ("v", ConfigValue::array([2.into()])),
        ("m", ConfigValue::map([("x", 2.into())])),
        ("s", 2.into()),
        (
            "named",
            ConfigValue::array([ConfigValue::map([("name", "a".into()), ("q", 2.into())])]),
        ),
    ])
}

fn summary(value: &ConfigValue) -> (Vec<u64>, Option<u64>, Option<u64>, Option<u64>, usize) {
    let v = value
        .get("v")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .filter_map(ConfigValue::as_u64)
        .collect();
    let m = value.get("m").unwrap();
    let named = value.get("named").unwrap().as_array().unwrap();
    (
        v,
        m.get("x").and_then(ConfigValue::as_u64),
        m.get("y").and_then(ConfigValue::as_u64),
        value.get("s").and_then(ConfigValue::as_u64),
        named.len(),
    )
}

fn run<C: CanMergeLayers + HasConfigValueType<ConfigValue = ConfigValue>>(
    context: &C,
) -> ConfigValue
where
    C::Error: core::fmt::Debug,
{
    context.merge_layers(base(), overlay()).unwrap()
}

#[test]
fn each_policy_merges_as_documented() {
    assert_eq!(
        summary(&run(&Deep)),
        (vec![2], Some(2), Some(1), Some(2), 1)
    );
    assert_eq!(
        summary(&run(&Shallow)),
        (vec![2], Some(2), None, Some(2), 1)
    );
    assert_eq!(
        summary(&run(&Append)),
        (vec![1, 2], Some(2), Some(1), Some(2), 2)
    );
    assert_eq!(
        summary(&run(&Admerged)),
        (vec![1, 2], Some(2), Some(1), Some(2), 2)
    );
    assert_eq!(
        summary(&run(&Join)),
        (vec![1], Some(1), Some(1), Some(1), 1)
    );
    assert_eq!(
        summary(&run(&Adjoin)),
        (vec![1, 2], Some(1), Some(1), Some(1), 2)
    );
    let by_name = run(&ByName);
    // Scalar elements have no key, so they never match and are appended.
    assert_eq!(
        summary(&by_name),
        (vec![1, 2], Some(2), Some(1), Some(2), 1)
    );
    let merged = &by_name.get("named").unwrap().as_array().unwrap()[0];
    assert!(merged.get("p").is_some() && merged.get("q").is_some());
}

#[test]
fn merge_limits_come_from_the_context() {
    let deep = (0..70).fold(ConfigValue::from(1), |inner, _| {
        ConfigValue::map([("k", inner)])
    });
    let error = Deep
        .merge_layers(ConfigValue::empty_map(), deep)
        .unwrap_err();
    assert_eq!(error.to_string(), "limit exceeded: max_depth (64)");
}

#[test]
fn ignore_policy_drops_unknown_keys() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Only {
        s: u8,
    }
    let only: Only = Deep.deserialize_config(base()).unwrap();
    assert_eq!(only, Only { s: 1 });
    let error = CanDeserializeConfig::<Only>::deserialize_config(&Deep, ConfigValue::empty_map())
        .unwrap_err();
    assert_eq!(error.code(), "config::missing");
}
