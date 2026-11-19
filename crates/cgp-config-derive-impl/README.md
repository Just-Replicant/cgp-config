# cgp-config-derive-impl

The implementation of `#[derive(ConfigSection)]` for [cgp-config](../../README.md),
as an ordinary library over `proc_macro2` token streams. The proc-macro crate
`cgp-config-derive` is a thin wrapper around [`derive_config_section`]; applications
use the derive through the facade (`cgp_config::ConfigSection`).

Keeping the logic here lets it be unit-tested without a compiler round trip and fuzzed
(`fuzz/fuzz_targets/derive_input.rs`): any input must produce either tokens or
`compile_error!`s, never a panic.

The attribute reference lives in the facade's documentation (`cgp_config::derive`).

License: AGPL-3.0-or-later.
