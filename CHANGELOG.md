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

[Unreleased]: https://github.com/Just-Replicant/cgp-config/commits/main
