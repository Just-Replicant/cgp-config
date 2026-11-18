# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Workspace scaffolding (P0): edition 2024, MSRV 1.99, toolchain 1.99.1, workspace lints,
  `rustfmt`, `cargo-deny`, `release-plz`, GitHub Actions CI and nightly jobs, and an
  `xtask` runner (`ci`, `fmt`, `clippy`, `test`, `hack`, `nostd`, `docs`, `deny`, `miri`,
  `coverage`).
- ADR-001 recording the prototype findings and where the design plan supersedes them.
- `cgp-config-core` (P1, `#![no_std]`): the component set (`HasConfigValueType`,
  `HasEmptyConfigValue`, `CanLoadSource`, `CanPrepareLayer`, `CanMergeLayers`,
  `CanCollectLayers`, `CanInterpolate`, `CanDeserializeConfig`, `CanResolveSecrets`,
  `CanValidateConfig`, `CanLoadConfig`, `HasLoadLimits`, `CanReportWarning`/`HasLoadReport`,
  `HasActiveProfile`, `CanParseFormat`/`CanRenderFormat`, and the 0.2 reload traits);
  `KeyPath`, `Origin`, `Span`, `Meta` with a capped provenance chain; `ConfigError` with
  stable codes; `ValidationErrors`; `RedactedRef`; `LoadReport` and `LoadWarning`;
  `LoadLimits`; the `LoadLayered` / `CollectInOrder` pipeline providers; and
  `config_check!`.
- `cgp-config-value` (P1): `ConfigValue` with `Arc` nodes and per-node `Meta`; `ConfigMap`
  (ordered); `Number`; fallible merge with deep, shallow, append, base-wins, and by-key
  policies; a type-directed serde deserializer that coerces env strings, reports paths and
  origins, collects unknown keys with suggestions, and redacts secrets; serde support for
  the tree; `CoreConfigPreset`.
- `cgp-config` facade with `prelude` and pass-through features for the fork.
- Built-in source markers in `cgp_config_core::markers` (`Defaults`, `ConfigFile`,
  `ConfigDir`, `ProfileFile`, `LocalFile`, `Embedded`, `EnvVars`, `EnvDocument`, `DotEnv`,
  `CliArgs`, `InMemory`, `DynSources`), re-exported by the prelude.
- `PrepareFileLayer`: records and strips `$version`, strips `$include` with a warning, and
  expands `$profiles.default` then `$profiles.{active}`; it is the only provider that
  requires `HasActiveProfile`.
- `DefaultPreparers`, the `UseDelegate` table behind `CoreConfigPreset`'s layer preparer.
- `CanReportWarning::clear_warnings`, called by `LoadLayered` at the start of every load.
- `ConfigError::unknown_keys` and `UnknownKey`; `from_value_with_limits`;
  `MergeOptions::max_depth`, `with_max_depth`, and `with_limits`; `map::IntoIter`.
- `cargo xtask semver` (cargo-semver-checks, advisory CI job on pull requests) and
  `cargo xtask fork-main` (nightly job building against `cgp-fork` main).
- Regression and property tests for every fix below, including a 200 000-level merge on a
  256 KiB thread and a `KeyPath` display/parse round-trip property.

### Changed

- **Breaking:** `CoreConfigPreset` wires `LayerPreparerComponent: UseDelegate<DefaultPreparers>`
  instead of `IdentityLayer`. Contexts using their own source markers add their own
  `UseDelegate` table; a marker without a preparer is a compile error. ADR-001 records the
  decision ("Preset shape fixed before P2").
- **Breaking:** `ConfigError::Unknown` holds every unknown key (`keys: Vec<UnknownKey>`);
  `DenyUnknownKeys` reports all of them in one error. The code stays `config::unknown`.
- **Breaking:** `SerdeWithProvenance` requires `HasLoadLimits`; `LoadLayered` requires
  `CanReportWarning` when `alloc` is on.
- `ConfigMap` uses a randomly seeded hasher (`foldhash::fast::RandomState`) and its
  `IntoIterator` returns the `map::IntoIter` wrapper, so neither leaks into the public API.
- Map keys parse `bool` case-insensitively and `f32`/`f64` like values do.
- Float coercion rejects values that are not finite (`"inf"`, `"NaN"`, overflow, and
  `f64` to `f32` narrowing that overflows).
- Miri runs with isolation on and `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`; the coverage
  gate applies to `cgp-config-value` only (core is printed).
- `cargo-deny` allows `BSD-3-Clause`.
- `config-crate-plan.md` points to the single canonical plan, `docs/design-plan.md`.

### Fixed

- Merging maps keeps the winning layer's origin on the merged container (`Meta::shadow`)
  instead of the base's, so the root names the last layer; merging onto or with an empty
  map returns the other side's `Arc` unchanged. `MergeArraysByKey` no longer drops the
  overlay's metadata.
- A key named `(root)` is quoted, so `KeyPath` display and parse round-trip.
- Merges stop at `max_depth` with `LimitExceeded` before recursing, instead of overflowing
  the stack; dropping a very deep tree no longer recurses.
- JSON array text in env and CLI values is parsed under the context's `LoadLimits`
  (depth counted from the node's position, keys shared with the rest of the tree).
- A second load no longer stacks warnings on the first load's report.
- `DenySecrets` documents that it is a no-op until the `secrets` feature.
- `Span::new` no longer produces an inverted span when `end < start`.
- Documented that `deserialize_any` targets (untagged enums, `flatten`) see env strings
  as strings.

[Unreleased]: https://github.com/Just-Replicant/cgp-config/commits/main
