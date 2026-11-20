# ADR-002: Phase 2 and 3 implementation choices

* Status: Accepted
* Date: 2026-11-19
* Phase: P2 (formats, sources, presets) and P3 (derive, validation)
* Design plan: [`docs/design-plan.md`](../design-plan.md) (§18 and the sections it cites)

## Context

P2 and P3 follow the design plan. Building them showed places where the plan left a
choice open, or where following it literally did not compose with the P1 preset. This
record lists those choices so later phases do not undo them by accident.

## Decision

1. **The root schema is a component.** `RootConfigMetaComponent` (`HasRootConfigMeta`)
   returns `Option<&'static ConfigMeta>`. `CoreConfigPreset` wires `NoConfigMeta`; a
   context names its type with `UseConfigMeta<AppConfig>`. Defaults, env mapping, file
   canonicalization, and deserialization all read the same answer, so a context that
   never derives `ConfigSection` pays nothing.
2. **Every layer is canonical before merging.** File documents (and each `$profiles`
   table), env, `.env`, and CLI layers are rewritten to serde keys, with flattened
   sections nested under their field, before `merge`. Merging is then key by key, an alias
   in a file overrides a default under the current name, and `normalize` after merging
   only fills what is still missing. The alternative (normalizing once after merging)
   reported `DuplicateKey` for `alias` versus default and let flattened defaults override
   file values.
3. **`ConfigFile` picks its format by extension** (`LoadFileAuto`); a searched file is
   optional, an explicit one (`--config`, `{PREFIX}_CONFIG`) is required. Search order:
   explicit path, `$CONFIGURATION_DIRECTORY` (wins over the working directory), the working
   directory, the executable's directory, the platform config directory. **A search that
   finds nothing makes the layer absent** (`Ok(None)`, logged as `LayerSkipped`), not
   `ConfigError::Missing`: a twelve-factor app configured only from env must start.
   `Bootstrap::require_file(true)` turns the empty search into `ConfigError::Missing`
   listing every searched path. `$CONFIGURATION_DIRECTORY` is split on `:` on Unix only
   (systemd joins several directories that way) and taken as one path elsewhere, where
   `:` is part of a drive letter; the first entry is used.
4. **`conf.d` files are prepared one by one** as `ConfigFile` documents, so
   `DefaultPreparers` maps `ConfigDir` to `IdentityLayer`.
5. **The `.env` parser is ours.** `dotenvy` substitutes `${VAR}` from the process
   environment, which breaks hermetic tests and the "one env snapshot" rule. The parser
   supports `export`, quotes, escapes, and multi-line double-quoted values, never
   substitutes, and errors name only the line, never the value. The `dotenv` feature
   therefore has no dependency.
6. **`Origin::DotEnv` holds an `Arc<DotEnvSource>`** so `Origin` stays two words.
7. **Missing fields are aggregated, not tracked with a bitmask.** The schema pass appends
   a `ConfigError::Missing` for every required field it does not find (in every section,
   at every depth, including flattened and list elements) to one `AggregateError`, together
   with the duration, byte-size, and duplicate-alias failures. An earlier draft used a
   per-section `u64` bitmask; it capped a section at 64 fields and could not name nested
   paths, so it was dropped. With `deny_unknown`, unknown keys join the same aggregate, so
   one load reports both a typo and the field it was meant to be.
8. **The derive accepts named-field structs only.** Enums and tuple structs are a
   spanned compile error; `#[config(flatten)]` is implemented by restructuring the tree,
   not by serde's `flatten` (which loses spans and coercion).
9. **The format parsers enforce `max_bytes`** themselves, so in-memory documents
   (`from_toml_str`, `{PREFIX}_DOCUMENT`) are bounded like files.

10. **Profile and local file naming.** The profile file is `{stem}.{profile}.{ext}` and
    the local file `{stem}.local.{ext}`, both beside the main file and in the main file's
    format: `app.toml` → `app.production.toml`, `app.local.toml`; `/etc/svc/config.json` →
    `config.production.json`. Without a main file (none searched, none explicit) there
    are no siblings. The profile name `local` is therefore reserved. The main file's
    `$profiles` table is recorded while `PrepareFileLayer` expands it
    (`report_profile_table`), so `LoadProfileFile` does not read or parse the main file a
    second time to learn whether the active profile exists.
11. **`RequireActiveProfile` is a source loader, not a preparer.** A preparer only sees a
    layer that exists, so it cannot see a missing profile file. `RequireActiveProfile`
    loads `ProfileFile` like `LoadProfileFile` and fails with
    `ConfigError::MissingProfile` when neither the profile file nor a `$profiles.{name}`
    table recorded for the main file exists (`default` never fails). `StrictPreset`
    extends `TwelveFactorPreset` with it and with `SerdeWithProvenance<DenyUnknownKeys>`
    (design plan §6.15). It relies on `ConfigFile` coming before `ProfileFile`, which every
    shipped layer list guarantees.
12. **Flatten collisions are errors twice.** At compile time the derive emits
    `ConfigMeta::checked`, a `const fn` that fails the build when two visible keys
    (fields, aliases, flattened children, or their env names after folding) collide. At
    load time the schema pass reports a key written both on the parent and under the
    flattened field as `DuplicateKey` with both spans; a key claimed by two flattened
    sections, or a flattened field that is not a table, is `ConfigError::Type`.
13. **`--set` with an index patches one element.** `--set servers[1].host=x` builds an
    index patch (`Meta::index_patch`) that the merge applies to element 1 of the lower
    array, leaving the other elements and the other keys of element 1 untouched. Index
    `len` appends; a gap is a type error naming the length; a patch over a scalar is a
    `MergeConflict`. `merge` materializes any patch left at the end (no lower array) into
    a plain array; indices are capped by `MAX_OVERRIDE_INDEX`.
14. **The loader refuses bootstrap inputs its layer list would drop.** `StdLoader<Layers>`
    names its layer list as a type (`cgp_config_providers::layers`). `StdLoader::new`
    fails when `--set` overrides were given and the list has no `CliArgs`, when
    `{PREFIX}_DOCUMENT` is set and the list has no `EnvDocument`, and when an explicit
    `.env` path was given and the list has no `DotEnv`. With `DotEnv` and no explicit path
    the default `./.env` is used. `Bootstrap::from_args` rejects `--set` unless the caller
    declared `CliArgs` (`Bootstrap::layers::<L>()`); the clap integration reads
    `--env-prefix` and offers `config_command()` (`template`, `schema`, `explain`,
    `check`).
15. **`${ref:…}` cannot copy a secret.** The interpolator refuses a reference to any
    field the root schema marks `secret` (or a node already marked secret), so a secret
    cannot be laundered into a plain field. Secret detection on file contents for the
    permission warning matches every `${scheme:…}` reference scheme in
    `SECRET_SCHEMES` and `secret://`; schema-driven detection of literal secret values
    in files waits for P4.
16. **Every layer is canonical.** `EnvDocument` (`{PREFIX}_DOCUMENT`) is canonicalized
    against the root schema like files, env, and CLI layers.
17. **Env names fold to `snake_case`.** `APP__MAX_CONNECTIONS`, `APP__MAXCONNECTIONS`, and
    `APP__maxConnections` match the field `max_connections` (and a `camelCase` serde
    name); a fold that matches two fields is `ConfigError::AmbiguousEnv`. A single `_`
    separator (`LossyUnderscore`) is **not available** in 0.1: it is rejected, and the
    rustdoc of `LoadEnvVars` says so.
18. **Duplicate keys carry both spans** in TOML and JSON (`DuplicateKey { first, … }`,
    displayed as `… (first at file:line:col)`). TOML uses `toml_parser` events to find the
    first occurrence, including dotted keys, inline tables, and arrays of tables.
19. **File reads.** A read is torn only when the length changes between `open` and the
    end of the read, or the bytes read differ from it; modification times are not
    compared (coarse clocks gave false retries). In `conf.d` a symlink is followed only
    when it resolves to a regular file inside the canonical `conf.d` directory; others
    are skipped. Only variables cgp-config reads (`{PREFIX}__…`, `{PREFIX}_CONFIG`,
    `_PROFILE`, `_DOCUMENT`, `CONFIGURATION_DIRECTORY`) fail on non-UTF-8 values.
20. **In-memory loads.** `from_toml_str_with` / `from_json_str_with` take a secret resolver
    and return `Loaded { config, warnings }`; `TestConfigPreset` (version 2) extends
    `CoreConfigPreset` with `Defaults`, `InMemory`, `EnvVars` (`TestLayers`) and
    `NoopLogger`.

Left for later phases: `quick::load`, partial loads, and generated getters (P5); secrets
resolution, mock secret schemes for `TestConfigPreset`, and schema-driven secret detection
in files (P4); JSON Schema output for `config schema` (it prints a field table in 0.1).

## Consequences

`PrepareFileLayer` and `SerdeWithProvenance` gained the `HasRootConfigMeta` bound
(breaking for contexts that do not use `CoreConfigPreset`; they add
`RootConfigMetaComponent: NoConfigMeta`). Custom sources should canonicalize their layer
with `cgp_config_value::schema::canonicalize` when a schema is present.

The hardening pass (items 3, 7, 10–20) is breaking in places: `StdLoader::new` and
`StdLoader::for_section` return `Result`; `StdLoader` takes a layer-list parameter
(default `TwelveFactorLayers`); `Bootstrap::from_args` rejects `--set` unless `CliArgs`
is declared; `ConfigError::DuplicateKey` gained `first`; `CanReportWarning` gained
`report_profile_table` / `profile_table_reported` (custom reporters implement them; the
shipped `ReportOnLoader` and `IgnoreWarnings` do); `TestConfigPreset` is now based on
`CoreConfigPreset` with a fixed layer list.
