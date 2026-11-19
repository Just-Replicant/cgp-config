//! # cgp-config-value
//!
//! The default value tree of [`cgp-config`](https://docs.rs/cgp-config) and everything
//! that operates on it without IO:
//!
//! * [`ConfigValue`]: reference-counted nodes, each with [`Kind`] data and
//!   [`Meta`](cgp_config_core::Meta) provenance;
//! * [`merge`](merge::merge): fallible, provenance-recording merge, and the merge providers
//!   ([`DeepMerge`], [`ShallowReplace`], [`AppendArrays`], [`JoinBaseWins`],
//!   [`AdjoinArrays`], [`MergeArraysByKey`]);
//! * [`de`]: a serde `Deserializer` that coerces env strings by target type, reports
//!   errors with key path and origin, never echoes secret input, and collects unknown keys;
//! * [`schema`]: the passes driven by `#[derive(ConfigSection)]` metadata (const
//!   defaults, flatten, aliases, mounts, durations, byte sizes, secrets, per-field merge
//!   policies, per-section `deny_unknown`);
//! * [`interpolate`] (feature `interpolate`): `${VAR}` / `${ref:a.b}` expansion of
//!   opted-in fields;
//! * [`CoreConfigPreset`]: the wiring every loader context shares.
//!
//! `#![no_std]` with `alloc`. No filesystem and no environment access: sources live in
//! the std providers crate.
#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

pub mod de;
mod defaults;
#[cfg(feature = "interpolate")]
#[cfg_attr(docsrs, doc(cfg(feature = "interpolate")))]
pub mod interpolate;
pub mod map;
pub mod merge;
mod number;
mod prepare;
pub mod preset;
mod providers;
pub mod schema;
mod serde_impls;
mod value;

pub use de::{
    DeserializeError, UnknownKey, from_value, from_value_with_limits, from_value_with_unknown,
};
pub use defaults::{LoadDefaults, LoadDefaultsOf};
#[cfg(feature = "interpolate")]
pub use interpolate::InterpolateOptedIn;
pub use map::ConfigMap;
pub use number::Number;
pub use prepare::PrepareFileLayer;
pub use preset::{CoreConfigPreset, DefaultPreparers};
pub use providers::{
    AdjoinArrays, Admerge, AppendArrays, DeepMerge, DenyUnknownKeys, EmptyMap, IgnoreUnknownKeys,
    JoinBaseWins, KeyedMerge, MergeArraysByKey, SerdeWithProvenance, ShallowReplace,
    UnknownKeyPolicy, WarnUnknownKeys,
};
pub use value::{ConfigValue, Kind};

/// The text that replaces secret values in `Debug`, dumps, and redacted copies.
pub const REDACTED: &str = "[REDACTED]";
