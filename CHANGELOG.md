# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **P2 — formats, sources, presets.**
  - `cgp-config-formats` (`#![no_std]` + `alloc`): TOML (feature `toml`, default) and a
    strict JSON parser (feature `json`) with a span on every node, duplicate keys
    rejected, `max_bytes`/`max_depth`/`max_keys` enforced while parsing, and redacting
    renderers; `TomlFormat`, `JsonFormat`, `EnabledFormats`, `FormatKind`, `LineIndex`.
    A conformance suite checks both formats against the same documents.
  - `cgp-config-providers` (std): `Bootstrap` (env prefix, one env snapshot, profile,
    config location, `.env` path, `--config`/`--profile`/`--env-prefix`/`--set`; `clap`
    integration behind `clap`), `ConfigLocation`/`SearchPath` (explicit path, then
    `$CONFIGURATION_DIRECTORY`, the working directory, the executable's directory, the
    platform config directory), file sources (`LoadFile<Fmt>`, `LoadFileAuto`,
    `LoadProfileFile`, `LoadLocalFile`, `LoadConfDir`, `LoadEmbedded<Fmt>`),
    `ReadStdFile` with a torn-read retry and Unix permission warnings, `ReadFromMap` for
    tests, `LoadEnvVars` (`{PREFIX}__A__B`, `env_raw` names), `LoadEnvDocument`
    (`{PREFIX}_DOCUMENT`, `json`), `LoadDotenv` with a hermetic `.env` parser (`dotenv`),
    `LoadCliOverrides`, `LoadFromField`, `LoadDynSources`, the `StdSources` table, and the
    presets `TwelveFactorPreset`, `TwelveFactorLocal`, `TwelveFactorConfDir`,
    `TwelveFactorDotenv`, `TwelveFactorCli`, `TwelveFactorDotenvCli`,
    `InterpolatingPreset`, `TestConfigPreset`; `StdLoader`, `MemoryLoader`,
    `from_toml_str`, `from_json_str`, `from_defaults`.
  - `cgp-config-core`: `EnvSnapshot`, `HasEnvSnapshot`, `HasEnvPrefix`;
    `HasRootConfigMeta` (`RootConfigMetaComponent`, `NoConfigMeta`, `UseConfigMeta<T>`);
    `CanLog` events (`SourceLoaded`, `LayerSkipped`) with `NoopLogger`;
    `Origin::DotEnv`.
- **P3 — derive and validation.**
  - `#[derive(ConfigSection)]` (`cgp-config-derive`, `cgp-config-derive-impl`): typed
    const defaults, `parse = "duration" | "bytes"`, `nested`, `flatten`, `rename`/
    `alias`/`rename_all` (serde attributes honoured), `env`/`env_raw`, `secret`,
    `interpolate`, per-section `deny_unknown`, doc comments as descriptions; misuse is a
    spanned compile error (trybuild suite).
  - `cgp-config-core`: `ConfigSection`, `ConfigMeta`, `FieldMeta`, the duration and
    byte-size parsers (`30` is rejected, `30s` parses), `AggregateError`,
    `ValidationRule`, `ValidateFn`, `AllOf`, `ValidateSections`, `Nested`.
  - `cgp-config-value`: the schema pass (`schema::canonicalize`, `normalize`): defaults
    from `ConfigMeta`, flatten, aliases, mounts, unit parsing, secret marking, every
    missing required field reported at once; `KeyedMerge`; `LoadDefaults`;
    `InterpolateOptedIn` (feature `interpolate`; `${` in fields that did not opt in is
    left as written).
  - `ValidateWithGarde` (`validate-garde`) and `ValidateWithValidator`
    (`validate-validator`) adapters.
  - `fuzz/` (cargo-fuzz, outside the workspace): TOML and JSON parse/render round-trip,
    the interpolation lexer, env mapping, the `.env` parser, and derive input.
- The facade re-exports all of the above behind matching features (default:
  `std`, `derive`, `toml`); `xtask` checks, documents, and semver-checks the new crates.

### Review hardening (`fix/review-hardening`)

See ADR-002 items 3, 7, and 10–20.

#### Added

- `cgp_config_providers::layers`: named layer lists (`TwelveFactorLayers`,
  `TwelveFactorLocalLayers`, `TwelveFactorConfDirLayers`, `TwelveFactorCliLayers`,
  `TwelveFactorDotenvLayers`, `TwelveFactorDotenvCliLayers`,
  `TwelveFactorDocumentLayers`, `TestLayers`) and the `LayerList` trait;
  `Bootstrap::layers::<L>()`, `Bootstrap::accepts_cli_overrides`,
  `StdLoader::with_layers`, `StdLoader::with_section`, `StdLoader::load_with`.
- `StrictPreset` (`DenyUnknownKeys` + `RequireActiveProfile`, design plan §6.15) and the
  `RequireActiveProfile` source loader.
- `from_toml_str_with` / `from_json_str_with` (secret resolver, `Loaded { config,
  warnings }`); unknown keys in `deny_unknown` sections are aggregated with missing
  fields.
- Clap: `--env-prefix` is read, `config_command()` adds `template`, `schema`, `explain`,
  and `check` subcommands (`ConfigCommand`).
- Compile-time key checks for derived sections (`ConfigMeta::checked`): clashes between
  fields, aliases, flattened children, and folded env names fail the build.
- `cgp_config_core::env_fold` (`snake_case` folding for env segments).
- `CanReportWarning::report_profile_table` / `profile_table_reported`.
- `Meta::index_patch` and `merge_keeping_patches` (array element patches).

#### Changed

- **Breaking:** `StdLoader::new` / `for_section` return `Result` and refuse bootstrap
  inputs the layer list would drop (`--set` without `CliArgs`, `{PREFIX}_DOCUMENT`
  without `EnvDocument`, an explicit `.env` path without `DotEnv`); `StdLoader` carries
  `env_document`, `dotenv_path`, and `cli_overrides`. `Bootstrap::from_args` rejects
  `--set` unless `CliArgs` is declared. With `DotEnv` and no explicit path, `./.env` is
  the default.
- **Breaking:** `ConfigError::DuplicateKey` carries the first occurrence; TOML and JSON
  report both spans.
- **Breaking:** `TestConfigPreset` (version 2) extends `CoreConfigPreset` with
  `TestSources`, `LoadLayered<TestLayers>`, and `NoopLogger`.
- A directory search that finds nothing makes the file layer absent unless
  `require_file(true)`.
- Env segments fold to `snake_case`; `LossyUnderscore` is documented as unavailable.
- `{PREFIX}_DOCUMENT` is canonicalized against the root schema like the other layers.
- `$CONFIGURATION_DIRECTORY` is split on `:` on Unix only.
- `capture_env` fails on non-UTF-8 values only for variables cgp-config reads.
- `conf.d` follows symlinks that resolve to a file inside the directory.
- Torn reads are detected by length only.
- The secret-reference heuristic for the permission warning covers every reference
  scheme (`SECRET_SCHEMES`, `secret://`).

#### Fixed

- `${ref:…}` could copy a `#[config(secret)]` field into a plain field; the interpolator
  now checks the schema.
- `--set servers[1].host=x` replaced the whole `servers` array; it now patches element 1.
- `LoadProfileFile` read and parsed the main file a second time.
- A key set both on a parent and under a flattened field, or claimed by two flattened
  sections, was silently merged.
- `StdLoader` silently dropped `{PREFIX}_DOCUMENT`, the `.env` path, and `--set`.

### Added (P0, P1)

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

- **Breaking:** `CoreConfigPreset` wires `RootConfigMetaComponent: NoConfigMeta`.
- **Breaking:** `PrepareFileLayer` requires `HasRootConfigMeta` and canonicalizes the
  document and every `$profiles` table against the root schema, so every layer has the
  same shape (serde keys, flattened sections under their field) before merging; an alias
  in a file overrides a default under the field's current name.
- **Breaking:** `SerdeWithProvenance` requires `HasRootConfigMeta` and `CanReportWarning`:
  with a schema it normalizes the tree first, and unknown keys in a `deny_unknown` section
  fail whatever the policy.
- `DefaultPreparers` maps `ConfigDir` to `IdentityLayer`: `LoadConfDir` prepares each
  `conf.d` file on its own.
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
