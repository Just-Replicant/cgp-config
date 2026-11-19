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
   directory, the executable's directory, the platform config directory.
4. **`conf.d` files are prepared one by one** as `ConfigFile` documents, so
   `DefaultPreparers` maps `ConfigDir` to `IdentityLayer`.
5. **The `.env` parser is ours.** `dotenvy` substitutes `${VAR}` from the process
   environment, which breaks hermetic tests and the "one env snapshot" rule. The parser
   supports `export`, quotes, escapes, and multi-line double-quoted values, never
   substitutes, and errors name only the line, never the value. The `dotenv` feature
   therefore has no dependency.
6. **`Origin::DotEnv` holds an `Arc<DotEnvSource>`** so `Origin` stays two words.
7. **Missing fields are tracked with a bitmask** per section while normalizing, so all
   missing required fields are reported in one error.
8. **The derive accepts named-field structs only.** Enums and tuple structs are a
   spanned compile error; `#[config(flatten)]` is implemented by restructuring the tree,
   not by serde's `flatten` (which loses spans and coercion).
9. **The format parsers enforce `max_bytes`** themselves, so in-memory documents
   (`from_toml_str`, `{PREFIX}_DOCUMENT`) are bounded like files.

Left for later phases: `quick::load`, partial loads, and generated getters (P5), secrets
resolution (P4).

## Consequences

`PrepareFileLayer` and `SerdeWithProvenance` gained the `HasRootConfigMeta` bound
(breaking for contexts that do not use `CoreConfigPreset`; they add
`RootConfigMetaComponent: NoConfigMeta`). Custom sources should canonicalize their layer
with `cgp_config_value::schema::canonicalize` when a schema is present.
