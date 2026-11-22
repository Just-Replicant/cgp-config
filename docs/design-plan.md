# `cgp-config` — design and implementation plan for a context-generic configuration crate on `cgp-fork`

Working name: `**cgp-config**` (Rust path `cgp_config`; the name was unused on crates.io when checked).

> **Basis.** Everything about CGP below is based on `Just-Replicant/cgp-fork` **v1.1.0** (tag `v1.1.0`, released **2026-11-02**; study snapshot `origin/main` @ `d504ba10`), studied in `/workspace/cgp-fork-main` (a detached worktree of `/workspace/cgp-fork`).
> The pre-existing checkout at `/workspace/cgp-fork` was on branch `phase-3-fixes` (v0.10.1). That is **not** the release line. All API citations are against v1.1.0.
> A throwaway prototype at `/workspace/cgp-config-proto` compiled the first cut of the components (`cargo test` on toolchain 1.99.1 and on MSRV 1.89). Appendix A lists what that prototype actually covered. The normative design in this document supersedes the prototype wherever they differ: pipeline order, fallible merge, file preparation, error mapping, interpolation, and reload publication. Those later sketches are designs; they have not been compiled yet. Sketches still marked *(validated)* match the prototype.

---

## 0. TL;DR

* **CGP is the loading machinery and the consumption API. Config data stays plain `serde`.** Loading is a fixed pipeline of small components: read, prepare, merge, interpolate, deserialize, resolve secrets, validate, publish. Each context (app, test, CLI, library harness) wires providers with `delegate_components!` or picks a `cgp_preset!`.
* **Libraries never load config and never touch the process environment or the filesystem.** A library exposes a section struct plus, optionally, a `#[cgp_getter]` (`HasHttpConfig`). Env names and defaults are declarative metadata. The app's loader is what reads them.
* **One pipeline for startup and reload.** Later layers win. File layers are migrated and profile-expanded before they merge. Interpolation runs only on fields opted in with `#[config(interpolate)]`. Secrets are resolved into `Secret<T>` after deserialize. Validation sees the resolved value. Reload publishes only when the diff is non-empty and no field is marked `reload = "restart"`.
* **Default stack is local and explicit.** `TwelveFactorPreset` is `Defaults`, `ConfigFile`, `ProfileFile`, `EnvVars`. Dotenv and CLI are further presets behind their features. Unknown keys warn. Interpolation is off until a field opts in.
* **Provenance on every node.** Errors look like `http.port: expected u16 at config/prod.toml:12:8 (overridden by env APP__HTTP__PORT)` and, with the `miette` feature, render as labeled spans. The value tree never holds secret bytes.
* `**cgp-config-core` is public, semver-stable API.** Apps depend on the facade. Libraries depend on the facade with `default-features = false` and `features = ["derive"]`. `cgp-config-derive-impl` has no stability promise.

Normative pipeline (startup and every reload):

1. **Bootstrap** the loader (path, profile name, env prefix) in ordinary code. This step does not call `CanLoadConfig`.
2. **Collect** sources in list order. Each file-origin layer is prepared, then merged. Merge is fallible. Preparation in 0.1 expands `$profiles` and records `$version`. Preparation in 0.2 runs the migration chain first, then expands `$profiles`.
3. **Interpolate** only paths marked `#[config(interpolate)]`. Mark expanded nodes so a second pass is a no-op.
4. **Deserialize** with type-directed coercion and the unknown-key policy.
5. **Resolve secrets** into `Secret<T>`. The tree keeps references.
6. **Validate** the resolved value. Aggregate every error.
7. **Publish** on reload only after the steps above succeed, the structural diff is non-empty, and no changed field is `reload = "restart"`. Otherwise keep the live config.

---

## 1. Findings: what `cgp-fork` actually provides (v1.1.0)

### 1.1 Repository facts

| Item | Value | Source |
|---|---|---|
| Version | 1.1.0 (released 2026-11-02, tag `v1.1.0`; study snapshot `d504ba10`) | `Cargo.toml` `[workspace.package]`, `CHANGELOG.md` |
| Edition / MSRV | 2024 / **1.89** (dev toolchain pinned to 1.99.1) | `Cargo.toml`, `rust-toolchain.toml` |
| Crates | `cgp-fork` (facade), `cgp-fork-macro`, `cgp-fork-macro-impl` (internal, no stability promise), plus test crates and `xtask` | `Cargo.toml` members |
| Upstream base | `contextgeneric/cgp` reviewed through `3a235198` (PR #292, 2026-11-08). Fork 0.10 ≈ upstream 0.8.0 plus fixes, versioned independently | `UPSTREAM_COMMIT`, README "Differences from upstream" |
| `no_std` | `#![no_std]`. Only `core` is needed with `default-features = false` | `crates/main/cgp-fork/src/lib.rs`, README |
| Docs | `book/` (guide, reference, one page per diagnostic, migration), ADRs 0001–0009 in `docs/adr/`, plus the external `cgp-knowledge-base-fork` | README, `docs/adr/` |
| License | Apache-2.0 | `LICENSE` |

**Feature flags** (`crates/main/cgp-fork/Cargo.toml`):

| Feature | Default | What it enables |
|---|---|---|
| `full` | yes | `alloc` and `compat-0.10` |
| `alloc` | via `full` | `DebugError` / `DisplayError` providers |
| `compat-0.10` | via `full` | deprecated 0.10 handler names |
| `anyhow` / `eyre` / `std-error` | no | `anyhow::Error` / `eyre::Report` / boxed `core::error::Error` as the context error |
| `tokio` / `smol` | no | `AsyncRuntime` adapters (spawn, sleep, timeout) |
| `mock` | no | `#[cgp_component(Name, mock)]` |
| `log-tracing` / `log-defmt` | no | `CanLog` backends |
| `inspect` | no | wiring JSON consts for `cargo xtask wiring` |

### 1.2 Constructs the plan relies on (all exported from `cgp_fork::prelude`)

| Construct | What we use it for | Cited from |
|---|---|---|
| `#[cgp_component(Provider)]`, with options `name`/`provider`/`context` and `mock` | every config capability | `cgp-fork-macro/src/lib.rs` (`cgp_component` docs), `examples/mock.rs` |
| `#[derive_delegate(UseDelegate<Param>)]` on a component | per-type dispatch (per source marker, per config type, per preparer) | `core/error/traits/can_raise_error.rs`, `handler/components.rs` |
| `#[use_type(HasErrorType.Error)]` | write a bare `Error` in component signatures | `core/error/traits/can_raise_error.rs`, `extra/runtime/traits/can_timeout.rs` |
| `#[prefix(@path in DefaultNamespace)]`, `cgp_namespace!` | optional namespacing of our components | `core/error/traits/*.rs`, `tests/namespaces/` |
| `#[cgp_impl(new Name)]` / `#[cgp_impl(Name<..>)]`, `#[cgp_provider]`, `#[cgp_new_provider]` | providers | macro docs, README example |
| `#[cgp_type]` (generates `UseType<T>` and the `XOf<Ctx>` alias) | abstract value and path types | `core/error/traits/has_error_type.rs` |
| `#[cgp_getter]` (generates `UseField<Tag>`, `UseFields`, `WithProvider`) / `#[cgp_auto_getter]` | library section accessors for contexts that own the config for the borrow | macro docs, `tests/getters/*` |
| `#[derive(HasField)]`, `#[derive(CgpData)]` (`HasBuilder`, `BuildField`, `FinalizeBuild`, `HasFields`) | loader contexts; typed partial builders | `core/field/traits/*.rs` |
| `HasOptionalBuilder`, `FinalizeOptional`, `CanBuildWithDefault`, `SetOptional` | typed partial config: every field `Option`, fill from defaults, report the first missing field by its `&'static str` name | `extra/field/impls/*.rs` |
| `delegate_components!` (`preset X`, `open C;`, `@C.Key: P`, `UseDelegate<new Table {..}>`, `type T = X`, `getter name`, `#[unchecked]`, `#[cfg_wiring(pred)]`) | wiring | macro docs, `examples/error_backends.rs`, `examples/profile.rs`, `tests/namespaces/open_dispatch.rs` |
| `cgp_preset!` (parents `A: B + C`, `#[version(N)]`, `override`) | ready-made stacks | `examples/preset_override.rs`, `tests/presets/combine.rs` |
| `check_components!` / `delegate_and_check_components!` (`#[check_trait]`, `#[check_params]`, `#[skip_check]`) | compile-time wiring tests | macro docs |
| `HasErrorType`, `CanRaiseError<E>`, `CanWrapError<D>`, `ErrorOf<Ctx>`; backends `cgp_fork::{anyhow,eyre,std_error}` | abstract error handling | `core/error/traits/*.rs`, `examples/error_backends.rs` |
| `#[cgp_auto_error]` on an enum (derives `Display`, `Error`, and `From` via `#[error]`, `#[source]`, `#[from]`) or on an inherent impl (error providers) | `ConfigError` | `examples/error_enum.rs`, CHANGELOG 1.1.0 |
| `#[cgp_async]` / `#[cgp_async(Send)]` (replaces the deprecated `#[async_trait]`) | async sources and reload | macro docs at `cgp_async`, ADR 0007 |
| `extra::runtime`: `HasRuntimeType`, `HasRuntime`, `AsyncRuntime`, `HasAsyncRuntime`, `CanSleep`, `CanSpawn`, `CanTimeout`, `SleepWithRuntime`, `SpawnWithRuntime`, `TimeoutWithRuntime`, `TimeoutElapsed`; `cgp_fork::tokio`, `cgp_fork::smol` | watchers, polling remote sources, timeouts | `extra/runtime/{ops,providers,traits}`, `examples/runtime.rs` |
| `CanLog<Detail>` / `Logger` and `TracingLogger` | observability | `extra/log/traits/can_log.rs`, CHANGELOG 1.1.0 |
| `Symbol!`, `Product!`, `Cons`/`Nil`, `StaticString` | section keys and type-level source lists | `core/base_types`, `core/field` |

`cgp_fork::handler` is intentionally unused. Per-key policy is a normal component (§19).

`#[cgp_getter]` is for methods that return a reference borrowed from `&self`. The generated `UseField` provider converts the field with `AsRef`. It is the right tool for a context that owns the config for the lifetime of the borrow. It is the wrong tool for a value living behind `ArcSwap`. Reload uses `ConfigHandle::load() -> Arc<Config>` and a request-scoped context (§6.10, §6.13).

`#[version(N)]` on a preset stores `VERSION` on that preset's module (`examples/preset_override.rs`). It does not keep the previous behavior available under the same preset name. A behavior change is a new preset type (`TwelveFactorV2`). `override` is a child-preset entry. In a `delegate_components!` table, an explicit entry replaces a preset entry. `PresetContains` checks parent overrides (CHANGELOG 1.0).

`spawn_task` on `AsyncRuntime` detaches the task and discards a panic inside it (CHANGELOG 1.1.0). The reload loop is therefore an ordinary `async fn` the application spawns on its own runtime (§6.10).

On Rust 1.87 and later, `std::env::set_var` is `unsafe` because it races with concurrent reads. MSRV 1.99 inherits that. This crate never calls `set_var` and never calls `dotenv()`'s process-mutating loader.

### 1.3 Behavior verified by the prototype

1. **Checked-by-default wiring breaks generic-parameter components.** Since 1.0, a non-generic `delegate_components!` emits `__CanUse{Context}` with check params `()`. For `CanLoadSource<Source>`, `CanValidateConfig<Config>`, and `CanRaiseError<E>` wired through `UseDelegate<new Table{..}>`, that check asks for a `()` entry and fails (`LoaderSources has no wiring entry for ()`). **Rule:** contexts that wire parametric config components use `#[unchecked]` plus an explicit `check_components!` with real params, or `#[check_params]` / `#[skip_check]` keys in `delegate_and_check_components!`. The happy path is the `config_check!` macro in §6.16, which expands to that check with our standard parameters. Presets document this in their rustdoc.
2. **A type-level source list folds correctly.** A `Cons<Source, Rest>` / `Nil` provider over `Product![..]` works with `#[cgp_impl]`.
3. `**#[cgp_async(Send)]` gives `Send` futures** for a generic `CanReloadConfig<Config>` when the context is `Sync`. The future borrows `&self`, so it is **not `'static`** (documented on `cgp_async`). A spawned loop owns `Arc<Context>` and calls methods on `&*arc` inside the task.
4. **The `mock` feature** generates `MockX` / `XMock`. A test context stores `x_mock: XMock`. This works for secret stores and remote sources.
5. Everything compiles on **MSRV 1.89**.

### 1.4 Gaps and caveats in the fork that the plan works around

* **No serde integration.** Rewrite-plan §7.3 proposes a `serde` feature for `HasFields` records. v1.1.0 has none. `cgp-config` owns its serde glue. A fork PR is welcome and is not a blocker; the `no_std` path uses typed partial builders either way.
* `**compat-0.10` is in `default`.** We depend with `default-features = false, features = ["alloc"]` (plus opt-ins) so the deprecated names stay out and `no_std` stays possible. `cgp-config-core` depends without `alloc`.
* **ADR 0001 (hard fork) is still "Proposed"**, and fork versions are independent of upstream. That carries a semver and maintenance risk (§19).
* `CgpData` partial builders report only the **first** missing field (`FinalizeOptional` returns `Err(&'static str)`). Config needs every missing field. We add `FinalizeOptionalAll`. The `no_std` no-alloc tier stores the names in a `heapless::Vec` with a documented cap (32). The alloc tier uses `Vec`.
* `Symbol!` strings are capped at 255 characters, and type-level strings make error messages long. Section keys should be short.
* The `#[cgp_async]` futures are not `'static`. `AsyncRuntime` sleep and timeout futures are `Send + 'static`.
* `#[cgp_getter]` cannot return `Arc<T>` through the generated `UseField` provider. Reload-aware code uses `ConfigHandle` plus a request context whose getters return references.

---

## 2. Ecosystem review: what to borrow and where CGP adds value

| Crate (version checked) | What we borrow | What we refuse to inherit |
|---|---|---|
| **config** 0.15.x | layered sources, env prefix, async source idea | stringly `Value` as the public config, case-folding surprises, a process-global builder, source-time comma splitting |
| **figment** 0.10.x | per-value `Metadata`, explicit merge/join/adjoin semantics, `Jail`'s *goal* of hermetic tests | runtime `dyn` as the only wiring, top-level profile keys that steal user names, process-global env mutation inside the jail |
| **confique** 0.4 | derive defaults, generated partials, commented templates | — |
| **twelf** 0.15 | 12-factor layering, clap as an input the app already parsed | — |
| **serde** + **serde_path_to_error** 0.1 | typed decoding, path of a failing key | echoing the raw value in errors for secret paths |
| **schemars** 1.x | JSON Schema 2020-12 (0.2, `schema` feature) | — |
| **toml** 1.x / **toml_edit** 0.25 | spans; comment-preserving TOML rewrites | — |
| YAML | a later format feature, backend isolated in `cgp-config-formats` | alias/anchor bombs; choosing `serde_norway` or a successor is deferred until that feature is implemented (§19) |
| **json5** 1.3, **ron** 0.12, **rust-ini** 0.21 | later format features | — |
| **dotenvy** 0.15 | parse a `.env` file into an owned map | `dotenv()` / anything that writes the process environment |
| **notify** 8 (stable; 9 stays out until it is stable) | file watching, via our `ChangeWatcher` | — |
| **arc-swap** 1.9 | lock-free `Arc<Config>` swap | — |
| **async-broadcast** | reload notifications that work on tokio and smol | a tokio-only broadcast type in the public handle |
| **secrecy** 0.10 | `SecretBox` / `SecretString`, zeroize, `Debug` prints `[REDACTED]` | — |
| **garde** 0.23 / **validator** 0.21 | the user's own derive on the struct; we provide an adapter provider | proxying their attribute syntax through our derive |
| **miette** 7.6 | span diagnostics behind a feature | a hard dependency |
| **clap** 4.6 | the app's already-parsed `ArgMatches`, behind a feature | calling `clap` parse inside a provider (that would steal argv) |
| **directories** | platform config dirs (`ProjectDirs`) | raw XDG paths on Windows |
| **indexmap** (alloc feature only) | stable key order for diffs and templates | — |

**Where CGP fills real gaps:**

1. **Per-context wiring at compile time.** Test, staging, and prod, or two binaries, choose sources and validators with no `cfg!` sprawl and no `dyn`. Providers are zero-cost static calls.
2. **Libraries without side effects.** A library states which section it needs. It does not read env and does not force a loader type on the app.
3. **Per-type dispatch** (`UseDelegate`): validators, file preparers, and secret schemes, each in one table.
4. **Mocks and cfg profiles** come built in (`mock`, `#[cfg_wiring(test)]`).
5. `**no_std` core.** Embedded apps reuse the traits with a postcard or flash source.
6. **Inspectable wiring** (`inspect`, `cargo xtask wiring`) documents which sources a binary actually uses.

Own the value tree. Figment and config-rs bridges are migration aids after 0.1, in a separate crate, and they are not the in-memory representation.

---

## 3. Goals and non-goals

### Goals

* G1 Typed, `serde`-based config for apps and libraries. Every source and behavior is swappable per context.
* G2 Layered sources with the precedence in §0. Profiles from bootstrap data plus `$profiles` and `{stem}.{profile}.{ext}` (e.g. `app.production.toml`).
* G3 Formats behind features. **0.1 ships TOML and JSON.** JSON5, RON, INI, and YAML are later features on the same `CanParseFormat` trait. Adding a format does not change app wiring.
* G4 Sources. **0.1:** defaults, an embedded document (`include_str!`), one file, search paths (cwd, executable directory, `CONFIGURATION_DIRECTORY`, platform dirs), `conf.d`, optional `{stem}.local.{ext}`, env, one optional JSON document env var, optional dotenv, optional clap, in-memory. **0.2:** root `$include`. **0.3+:** remote KV, cloud secret stores, and an optional decrypt wrapper, as satellite crates.
* G5 Interpolation of `${VAR}`, `${VAR:-default}`, `${VAR:?message}`, and `${ref:other.key}` on fields that opt in. Escape is `$$`. Cycles and depth are errors. Expanded nodes are not scanned again.
* G6 Validation: per-type, cross-field, nested sections, all errors at once.
* G7 Secrets: `Secret<T>` after deserialize, redacted `Debug` / logs / errors / dumps / diffs, eager by default, `LazySecret<T>` opt-in, zeroize on drop of the last `Arc`.
* G8 Hot reload (0.2): watch, debounce, short read-retry, resync poll, ignore editor scratch files, atomic swap, redacted diff, no notify when the diff is empty, no swap when a `restart` field changed, and a `RestartRequired` status a health check can read.
* G9 Async (0.2 for reload, trait sketched in 0.1 so it can land without a break): `#[cgp_async(Send)]`, tokio or smol via `AsyncRuntime`.
* G10 Schema and docs (0.2): JSON Schema, commented TOML templates, Markdown reference, env-var listing.
* G11 Diagnostics: key path, origin, span, override chain, "did you mean", stable `code()` string. Secret paths omit the raw text.
* G12 Sections from many crates, mounted by the app, without key or env-alias collisions.
* G13 `no_std` + `alloc` core. Facade default features are `std`, `derive`, and `toml` only.
* G14 File versioning and migrations (0.2), idempotent, TOML comment-preserving, crash-safe write.
* G15 Observability through `CanLog` and tracing events (layer loaded, layer skipped, reload applied, reload rejected, deprecated key). Events carry keys and origins, never values.

### Non-goals

* Not a CLI parser. We accept matches the app already parsed. `config_command()` returns a `clap::Command` the app can rename and attach. We do not auto-install a `config` subcommand.
* Not a secret manager. We resolve references. We do not store secrets.
* No runtime dylib plugins. The only dynamic escape hatch is `Box<dyn DynSource + Send + Sync>`.
* No CGP requirement for consumers of config structs. A struct remains usable with plain serde.
* No writes to remote stores. Local writes exist for migrations and templates.
* No mutation of the process environment. No `set_var`. No `dotenv()`.
* No secret bytes in the value tree, in `Meta`, in error `Display`, or in tracing fields.
* Cloud SDKs are not optional dependencies of the facade.
* Not a feature-flag service. A bool in config is a bool. Rollout, targeting, and experimentation stay in the app.
* Not an OS configuration store. No Windows registry provider and no plist provider.
* No "path must exist" check. Existence races and differs between a container build and the host. A validator the app writes can stat a path when the app wants that.
* No comma-separated env lists. Sequences are JSON text (§6.3).
* No age, sops, or KMS implementation inside the facade. A byte-decode wrapper is the extension point (§6.17).

---

## 4. Why CGP fits

Configuration is mostly **policy**: where values come from, in what order, how they merge, what is valid, what is secret, and how reload works. Policy differs per deployment context. The data (config structs) and the consumers (library code) stay free of that policy.

* **Consumer traits** (`CanLoadConfig<C>`, `HasHttpConfig`) are what code depends on.
* **Provider types** (`LoadFile<Toml>`, `LoadEnvVars`, `DeepMerge`, `ValidateWithGarde`) are reusable policies.
* **Contexts** (`ProdLoader`, `TestLoader`, `App`) choose policies in one `delegate_components!` table.
* **Presets** bundle common choices (`TwelveFactorPreset`). An explicit table entry replaces one preset entry. A child preset uses `override`.
* **Abstract types** (`HasErrorType`, `HasConfigValueType`) let `no_std` and custom trees swap the representation.

---

## 5. Workspace and crate layout

One facade, matching the fork. Internal crates exist for `no_std` boundaries and for proc-macro isolation.

```
cgp-config/                         (workspace; edition 2024; rust-version 1.99;
│                                    rust-toolchain.toml 1.99;
│                                    license AGPL-3.0-or-later; root LICENSE)
├─ crates/
│  ├─ cgp-config/                   facade: re-exports + prelude. Apps depend on this.
│  ├─ cgp-config-core/              PUBLIC. #![no_std]: components, KeyPath, error kind, limits
│  ├─ cgp-config-value/             PUBLIC. alloc: ConfigValue, Meta, merge, Deserializer
│  ├─ cgp-config-providers/         PUBLIC. std: files, env, dotenv, clap, interpolate, validate
│  ├─ cgp-config-formats/           PUBLIC. feature per format (0.1: toml, json)
│  ├─ cgp-config-secrets/           PUBLIC. Secret<T>, resolvers (file, env, docker)
│  ├─ cgp-config-derive/            PUBLIC proc-macro: #[derive(ConfigSection)]
│  ├─ cgp-config-derive-impl/       UNSTABLE. parse → validate → emit. Published only
│  │                                 because the proc-macro crate links it. Users do not
│  │                                 depend on it. Same rule as cgp-fork-macro-impl.
│  ├─ cgp-config-watch/             0.2. notify, ConfigHandle, reload loop
│  ├─ cgp-config-schema/            0.2. schemars, TOML templates, Markdown
│  └─ cgp-config-test/              publish = false. fixtures, golden diagnostics
├─ examples/                        publish = false
├─ book/
├─ benches/
├─ fuzz/
└─ xtask/                           publish = false
```

Satellite crates, **not** workspace members in 0.1 and **not** optional deps of the facade: `cgp-config-aws`, `cgp-config-gcp`, `cgp-config-vault`, `cgp-config-consul`, `cgp-config-etcd`, `cgp-config-http`. They implement the public resolver and source traits. They land at 0.3+.

`examples/`, `xtask/`, `benches/`, `fuzz/`, and `cgp-config-test` set `publish = false`.

Dependency on the fork, root `Cargo.toml`:

```toml
[workspace.dependencies]
cgp-fork = { version = "1.1", default-features = false, features = ["alloc"] }
```

`cgp-config-core` depends on `cgp-fork` with `default-features = false` and without `alloc`. The facade turns on `anyhow` / `eyre` / `std-error` / `tokio` / `smol` / `mock` / `log-tracing` / `inspect` through its own features (§15).

Who depends on what:

| Consumer | Dependency |
|---|---|
| Application | `cgp-config` (default features, plus the ones it uses) |
| Library with a section derive | `cgp-config` with `default-features = false, features = ["derive"]` |
| Library that only names traits | `cgp-config-core`, or the facade with `default-features = false` |
| Embedded `no_std` | `cgp-config-core` plus, when wanted, `cgp-config-value` |

Proc-macro path: like the fork (ADR 0003, `crates/tests/renamed-dep`), `cgp-config-derive` resolves the facade crate name from the caller's `Cargo.toml`, with a `CGP_CONFIG_CRATE` override. Renamed dependencies work.

---

## 6. Component design

Convention: consumer trait `CanX` / `HasX`, provider trait `Xer`, component `XerComponent`. Every component that is parametric over a type has `#[derive_delegate(UseDelegate<Param>)]`.

Built-in providers raise `ConfigError` only. An app implements `CanRaiseError<ConfigError>` once (directly, or via anyhow/eyre `From`). Apps do not list `std::io::Error`, `toml::de::Error`, or `serde_json::Error` in their wiring. A new format therefore adds no entry to existing contexts.

### 6.1 Core abstract types *(trait shape validated; bounds below are normative)*

```rust
use cgp_fork::prelude::*;

/// Intermediate tree. Default backend: cgp_config_value::ConfigValue.
/// `Clone` is cheap: nodes are `Arc`. There is no `Debug` bound.
/// Debugging goes through `RedactedDebug`, which prints `[REDACTED]`
/// for nodes whose `Meta::secret` is set and never prints secret bytes,
/// because secret bytes are not stored on nodes.
#[cgp_type]
pub trait HasConfigValueType {
    type ConfigValue: Clone;
}

/// Empty document for the fold. Provided next to the value type, not as a
/// free function on the context.
#[cgp_component(EmptyConfigValue)]
pub trait HasEmptyConfigValue: HasConfigValueType {
    fn empty_config_value(&self) -> Self::ConfigValue;
}

/// Profile name chosen at bootstrap. A `&str` into the loader's `String`.
#[cgp_auto_getter]
pub trait HasActiveProfile {
    fn active_profile(&self) -> &str;
}

/// Hard limits. Defaults: 10 MiB, depth 64, 100_000 keys,
/// interpolation depth 8, provenance chain 16.
#[cgp_auto_getter]
pub trait HasLoadLimits {
    fn load_limits(&self) -> LoadLimits;
}
```

`HasErrorType` / `CanRaiseError<E>` / `CanWrapError<D>` are the fork's. The error value built-in providers construct is always `ConfigError` (§6.11).

`LoadLimits` is a `Copy` struct in `cgp-config-core`. `DefaultLoadLimits` is the preset provider (a constant getter).

### 6.2 Bootstrap

`Loader::from_env_and_args` is ordinary Rust. It does not call `CanLoadConfig`, so the path and the profile do not depend on the file they select.

Settings it fills, and only these:

| Setting | CLI (when `clap` is on) | Env (read once into an owned map) | Built-in |
|---|---|---|---|
| config path | `--config` | `{PREFIX}_CONFIG` | search order in §6.3 |
| profile name | `--profile` | `{PREFIX}_PROFILE` | `"default"` |
| JSON document | — | `{PREFIX}_DOCUMENT` | unset means the layer is absent |
| env prefix | `--env-prefix` | — | uppercase app name, no trailing underscore (`myapp` → `MYAPP`) |

The prefix is `APP`, not `APP_`. Nested keys are `{PREFIX}__{PATH}` with the `__` separator: `APP__HTTP__PORT`. Bootstrap keys use a single underscore (`APP_CONFIG`, `APP_PROFILE`, `APP_DOCUMENT`). After bootstrap reads them, it deletes those three entries from the snapshot. The env layer only matches the `{PREFIX}__` prefix, so the bootstrap keys cannot also become config fields.

Precedence for these three settings is CLI, then env, then built-in. The profile name is never read from the config file.

The env read builds a `EnvSnapshot` (`BTreeMap<String, String>`). A non-UTF-8 value is `ConfigError::EnvEncoding` with the key name and without the bytes when the variable is one cgp-config reads (`{PREFIX}__…`, `{PREFIX}_CONFIG`, `{PREFIX}_PROFILE`, `{PREFIX}_DOCUMENT`, `CONFIGURATION_DIRECTORY`); other non-UTF-8 variables are skipped. `--set` is accepted only when the loader's layer list has `CliArgs`, and the loader refuses a `{PREFIX}_DOCUMENT` or an explicit `.env` path its layer list would ignore (ADR-002 §14). After bootstrap, every provider reads `HasEnvSnapshot`. Nothing reads `std::env` again, and nothing writes it.

`quick::load` uses this bootstrap and then `TwelveFactorPreset` (§13).

### 6.3 Sources

A **source marker type** identifies a layer. The provider is chosen per context.

```rust
#[cgp_component(SourceLoader)]
#[derive_delegate(UseDelegate<Source>)]
pub trait CanLoadSource<Source>: HasConfigValueType + HasErrorType {
    /// `Ok(None)` means this optional source is absent.
    /// A present source that fails to parse is `Err`.
    fn load_source(&self, source: PhantomData<Source>)
        -> Result<Option<Self::ConfigValue>, Self::Error>;
}
```

`PhantomData` is how the type parameter appears in the method. Callers that load a whole config use `CanLoadConfig` and do not pass markers.

**Absence vs error**

| Situation | Result |
|---|---|
| Optional file missing | `Ok(None)` |
| Required file missing | `Err(ConfigError::Missing)` |
| File present, empty TOML `""` or JSON `null`/`{}` as an empty document | `Ok(Some(empty map))` |
| Permission, IO, or parse error | `Err` |
| Directory search finds nothing and no explicit path was set | `Ok(None)` (the layer is absent). With `require_file(true)`: `Err(ConfigError::Missing)` listing the paths |

An explicit `config_path` is required. Search-path candidates are optional until one is found. `conf.d` is a different marker and merges every file.

**Built-in markers** (`cgp-config-providers`):

| Marker | Provider | Inputs (getters) |
|---|---|---|
| `Defaults` | `LoadDerivedDefaults<T>` from `ConfigMeta` (const default expressions). No `Serialize` bound | target type's generated meta |
| `ConfigFile` | `LoadFile<Fmt>` or `LoadFileAuto` | `HasConfigPath`, `HasFileRequired`, `CanReadConfigFile` |
| `ConfigDir` | `LoadConfDir<Fmt>` — every `conf.d/*.{ext}` in lexicographic UTF-8 order, each prepared and merged | `HasConfigSearchPaths`, `HasAppName` |
| `ProfileFile` | `LoadProfileFile` — `{stem}.{profile}.{ext}` beside the main file (`app.toml` → `app.production.toml`), optional when absent. `RequireActiveProfile` errors when neither it nor `$profiles.{name}` exists | `HasActiveProfile`, `HasConfigPath` (directory of the base file) |
| `EnvVars` | `LoadEnvVars` | `HasEnvPrefix`, `HasEnvSeparator` (default `"__"`), `HasEnvSnapshot`, `HasEnvKeyStyle` |
| `DotEnv` | `LoadDotenv` — parses a file into a layer. Feature `dotenv` | `HasDotenvPath` |
| `CliArgs` | `LoadClapMatches` reads `HasCliOverrides`. Feature `clap` | the app's already-parsed matches, projected into a map |
| `InMemory` | `LoadFromField<Tag>` | `HasField<Tag, Value = ConfigValue>` |
| `Embedded` | `LoadEmbedded<Fmt>` parses `include_str!` text. Optional; not in `TwelveFactorPreset` | `HasEmbeddedDocument` (`&'static str`) |
| `LocalFile` | `LoadLocalFile<Fmt>` — `{stem}.local.{ext}` next to the base file, optional when absent | `HasConfigPath` |
| `EnvDocument` | `LoadEnvDocument` — `{PREFIX}_DOCUMENT` (`APP_DOCUMENT`) is one JSON object, optional when unset. Feature `json` | `HasEnvPrefix`, `HasEnvSnapshot` |
| `DynSources` | `LoadDynSources` | `HasDynSources` |

`CanReadConfigFile` is the IO seam. `ReadStdFile` uses `std::fs`. `ReadFromMap` serves tests and never touches the disk. The reader enforces `LoadLimits::max_bytes` and returns `Ok(None)` on `NotFound`. `LoadFile` turns that `None` into `Missing` when the file is required. UTF-8 is required. A leading UTF-8 BOM is stripped. UTF-16 is `ConfigError::Encoding`.

The raw text is a `zeroize::Zeroizing<String>` until parse returns. On Unix the reader records `LoadWarning::WorldWritable` when the file is group-writable or world-writable. It records `LoadWarning::SecretWorldReadable` when the parsed layer contains a secret reference and the mode allows group or world read. A `0644` file with no secret reference does not warn. The reader retries once when the length changes between `open` and the end of the read, or the bytes read differ from it (a torn write); modification times are not compared. In `conf.d` a symlink is followed only when it resolves to a regular file inside the `conf.d` directory. Editor scratch names (`*~`, `*.swp`, `*.tmp`, `.#*`) are not config files; search and `conf.d` skip them.

`LoadFileAuto` maps the extension through a `match` on the enabled format features.

Search order used by bootstrap and by `quick::load`, via `directories::ProjectDirs` (on Windows this is the known-folder config dir, not XDG):

1. Explicit `--config` / `{PREFIX}_CONFIG` (`APP_CONFIG`) if set (required file). That variable is a **path**. The JSON blob is `{PREFIX}_DOCUMENT` (`APP_DOCUMENT`) (§6.17).
2. `{CONFIGURATION_DIRECTORY}/{app}.toml` when systemd set `CONFIGURATION_DIRECTORY` (on Unix the value is split on `:` and the first directory is used; elsewhere it is one path).
3. `./{app}.toml`, then `./config.toml`, then `./config/{app}.toml` (current directory).
4. The same three names in the directory of `std::env::current_exe()` (a service whose cwd is `/`).
5. `{project_config_dir}/{app}.toml`.

The first existing file wins for `ConfigFile`. `ConfigDir`, when the user adds it, additionally merges `{dir}/conf.d/*.toml` in sorted order after that file, skipping scratch names. `LocalFile` is a later layer, not part of this search (§6.17).

**Dynamic escape hatch.** Object-safe, fixed error type:

```rust
pub trait DynSource: Send + Sync {
    fn load(&self) -> Result<Option<ConfigValue>, ConfigError>;
}

pub trait HasDynSources {
    fn dyn_sources(&self) -> &[Box<dyn DynSource>];
}
```

`ConfigValue` here is the concrete alloc tree. Contexts with a custom tree do not use this hatch; they add a typed marker instead. The trait has no generic `Err` and no associated error type, so the box is object-safe.

**File loader shape** (the provider is not generic over every source):

```rust
#[cgp_impl(new LoadFile<Toml>)]
impl SourceLoader<ConfigFile>
where
    Self: CanReadConfigFile
        + CanParseFormat<Toml>
        + CanPrepareLayer<ConfigFile>
        + HasFileRequired
        + HasConfigPath
        + CanRaiseError<ConfigError>,
{
    fn load_source(&self, _: PhantomData<ConfigFile>) -> Result<Option<Self::ConfigValue>, Self::Error> {
        match self.read_config_file(self.config_path()) {
            Ok(None) if self.file_required() => Err(Self::raise_error(ConfigError::missing_path(self.config_path()))),
            Ok(None) => Ok(None),
            Ok(Some(text)) => self.parse_format(PhantomData::<Toml>, &origin, &text).map(Some),
            Err(e) => Err(e),
        }
    }
}
```

`read_config_file` and `parse_format` already return `ConfigError` (mapped inside those providers). Parse errors include origin and span. They do not include a copy of a secret-looking line beyond the span metadata; the diagnostic view loads source text from `SourceCache` under the `miette` feature, and secret paths suppress the snippet (§6.11).

**Env mapping**

* The layer stores **strings**. Numbers, bools, enums, and lists are parsed later, type-directed, during deserialize (§6.6).
* Default separator is `__`. The layer accepts only keys that start with `{PREFIX}__`. `APP__HTTP__PORT` with prefix `APP` becomes path `http.port`.
* A separator of a single `_` is rejected. `LossyUnderscore` (split against known `ConfigMeta` paths, prefer the unique match, `ConfigError::AmbiguousEnv` otherwise) is **not available in 0.1**; it may come later as a new key style. Env segments fold to `snake_case` before matching (`MAX_CONNECTIONS`, `MAXCONNECTIONS`, `maxConnections` all match `max_connections`); a fold that matches two fields is `ConfigError::AmbiguousEnv`.
* Env keys fold to `snake_case`. File keys stay case-sensitive. If the folded env key matches two file keys that differ only by case, the error is `ConfigError::AmbiguousEnv`.
* Lists are JSON array text (`[1,2]`, `["a","b"]`) when the target is a sequence. Comma splitting is not used.
* Bool accepts `true` and `false`, ASCII case-insensitive. Integers and floats use the Rust `FromStr` of the target type. A string that does not parse is a type error naming the path and the target type, without quoting the raw text when the path is secret.
* Empty string: kept for `String`; type error for `bool` / numbers / enums; empty sequence only when the text is `[]`.
* Present key with JSON `null` in a file is null. A missing key is absent. Null does not delete a value from a lower layer; it overrides that key with null. `Option<T>` is how a user represents absence after override.
* A `BTreeMap<String, T>` or `HashMap<String, T>` is a nested map. `APP__ROUTES__API__TIMEOUT` with prefix `APP` is `routes["api"].timeout`. The map-key segment is lowercased. File keys keep their original case. On Windows the process environment is case-insensitive, so two env keys that differ only by case are `AmbiguousEnv`.
* `PathBuf`, `IpAddr`, and `SocketAddr` parse with their `FromStr`. Durations and byte sizes use the parsers in §6.17. A bare number is not a duration.

**Clap.** `LoadClapMatches` reads `HasCliOverrides` (`BTreeMap<String, String>` of `a.b=value` the app extracted). The provider does not call `clap::Parser::parse`. The same coercion rules as env apply at deserialize time.

### 6.4 Formats (feature-gated)

```rust
#[cgp_component(FormatParser)]
#[derive_delegate(UseDelegate<Format>)]
pub trait CanParseFormat<Format>: HasConfigValueType + HasErrorType {
    fn parse_format(&self, _f: PhantomData<Format>, origin: &Origin, text: &str)
        -> Result<Self::ConfigValue, Self::Error>;
}

#[cgp_component(FormatRenderer)]
#[derive_delegate(UseDelegate<Format>)]
pub trait CanRenderFormat<Format>: HasConfigValueType + HasErrorType {
    fn render_format(&self, _f: PhantomData<Format>, value: &Self::ConfigValue)
        -> Result<String, Self::Error>;
}
```

0.1 markers: `Toml`, `Json`. Later markers (`Yaml`, `Json5`, `Ron`, `Ini`) can be added without changing the trait.

Rules for every parser:

* Duplicate keys are errors (`ConfigError::DuplicateKey` with both spans, in TOML and JSON: `duplicate key a.b at t.json:1:16 (first at t.json:1:8)`). JSON uses a reader that rejects duplicates rather than last-wins.
* Depth and key-count come from `HasLoadLimits`.
* YAML, when that feature exists, disables anchor and alias expansion so a document cannot blow up the process. The backend crate stays inside `cgp-config-formats`.
* Spans are recorded in `Meta` when the parser exposes them (`toml` spanned / `toml_edit`, JSON byte offsets).

Rendering is for templates, `dump`, and migrations. 0.1 implements TOML and JSON render. JSON rendering is lossy of comments because JSON has none.

### 6.5 Preparing a layer, merging, precedence

```rust
#[cgp_component(LayerPreparer)]
#[derive_delegate(UseDelegate<Source>)]
pub trait CanPrepareLayer<Source>: HasConfigValueType + HasErrorType + HasActiveProfile {
    fn prepare_layer(&self, _s: PhantomData<Source>, layer: Self::ConfigValue)
        -> Result<Self::ConfigValue, Self::Error>;
}

#[cgp_component(LayerMerger)]
pub trait CanMergeLayers: HasConfigValueType + HasErrorType {
    fn merge_layers(&self, base: Self::ConfigValue, overlay: Self::ConfigValue)
        -> Result<Self::ConfigValue, Self::Error>;
}

#[cgp_component(LayerCollector)]
pub trait CanCollectLayers<Sources>: HasConfigValueType + HasErrorType {
    fn collect_layers(&self, acc: Self::ConfigValue, _s: PhantomData<Sources>)
        -> Result<Self::ConfigValue, Self::Error>;
}
```

> **As built (ADR-001, "Preset shape fixed before P2"):** `CanPrepareLayer` has **no**
> `HasActiveProfile` supertrait. That getter needs `alloc` and an `active_profile: String`
> field, which `no_std` contexts and contexts without file layers lack, and adding the
> bound to the trait later would break every context. Only `PrepareFileLayer` requires
> `Self: HasActiveProfile`. `CoreConfigPreset` already wires
> `LayerPreparerComponent: UseDelegate<DefaultPreparers>` in P1.

`PrepareFileLayer` (wired for `ConfigFile`, `ProfileFile`, `ConfigDir`):

1. If `$version` is absent, record `LoadWarning::AssumedVersion` (the document is version 1). If `$version` is present, record it. 0.1 then removes the key so it does not become an unknown field.
2. **0.2:** run `CanMigrate<Config>` from that version up to the schema version (§7). 0.1 has no migration chain; this step is a no-op.
3. Expand `$profiles` (§6.5.1) and remove the `$profiles` key.
4. On nodes that came from the profile overlay, set the profile name. After a 0.2 migration, also set `Meta::migrated_from`.

`IdentityLayer` (wired for `Defaults`, `EnvVars`, `DotEnv`, `CliArgs`, `InMemory`, `DynSources`) returns the layer unchanged. Defaults are already the current schema. Env, dotenv, and CLI use current key names; old names from those layers are accepted only through `#[config(alias)]`.

```rust
#[cgp_impl(new CollectInOrder)]
impl<Source, Rest> LayerCollector<Cons<Source, Rest>>
where
    Self: CanLoadSource<Source> + CanPrepareLayer<Source> + CanMergeLayers + CanCollectLayers<Rest>,
{
    fn collect_layers(&self, acc: Self::ConfigValue, _: PhantomData<Cons<Source, Rest>>)
        -> Result<Self::ConfigValue, Self::Error>
    {
        let acc = match self.load_source(PhantomData::<Source>)? {
            Some(layer) => {
                let layer = self.prepare_layer(PhantomData::<Source>, layer)?;
                self.merge_layers(acc, layer)?
            }
            None => acc,
        };
        self.collect_layers(acc, PhantomData::<Rest>)
    }
}
```

`Nil` returns `acc`.

**Precedence is list order. Later wins.** The 0.1 default list is:

```text
Product![Defaults, ConfigFile, ProfileFile, EnvVars]
```

A type conflict (scalar merged onto a map, array merged onto a scalar, and the same for the reverse) is `ConfigError::MergeConflict` carrying both origins. It is not a silent replacement.

**Merge providers**

| Provider | Behavior |
|---|---|
| `DeepMerge` | maps merge recursively; scalars and arrays replace. Default |
| `ShallowReplace` | top-level keys replace |
| `AppendArrays` | arrays concatenate; everything else follows `DeepMerge` |
| `MergeArraysByKey<Key>` | array of maps is matched on `Key`. Missing key appends. Duplicate keys inside one layer are `DuplicateKey`. Stable order: base order, then keys that exist only in the overlay |
| `KeyedMerge<Policies>` | `UseDelegate` table, or the static table the derive emits from `#[config(merge = ...)]` |
| `JoinBaseWins` | figment join: base wins where both are present |
| `AdjoinArrays` / `Admerge` | figment's array-concat variants, explicit opt-in |

Every merged node keeps a provenance chain in `Meta`: winning origin and span, then up to `LoadLimits::max_provenance` shadowed origins, then a truncation flag. `Meta` lives in a side arena keyed by node id (§11). Nodes are `Arc`, so unchanged subtrees are shared.

#### 6.5.1 Profiles

Two mechanisms, both applied, with a defined order:

1. **Inside a file,** a reserved table `$profiles`. Sibling keys of `$profiles` are the shared base. `$profiles.default` is deep-merged onto that base, then `$profiles.{active}` is deep-merged on top. `$profiles` is removed before the layer merges, so those keys never reach deserialize.
2. **Beside a file,** `ProfileFile` loads `{stem}.{profile}.{ext}` (same directory and format as the main file: `app.toml` → `app.production.toml`; the name `local` is reserved for `{stem}.local.{ext}`) as a later layer. The whole file is the overlay. The same preparer runs, so a profile file may itself contain `$profiles`.

```toml
host = "0.0.0.0"

[http]
port = 8080

[$profiles.prod.http]
port = 80
```

Active profile `prod` yields `host = "0.0.0.0"`, `http.port = 80`.

The active profile comes from bootstrap (§6.2): CLI, then `{PREFIX}_PROFILE`, then `"default"`. It is not read from the document.

When the active name has no `$profiles.{name}` table and no profile file, `HasLoadReport` records a warning and the `default` table (if any) is what remains. Wiring `RequireActiveProfile` as the `ProfileFile` source loader turns that situation into `ConfigError::MissingProfile` (`StrictPreset` does). It reads the `$profiles` tables `PrepareFileLayer` recorded on the reporter while preparing the main file, so the main file is read once. The other presets use the warning behavior.

Top-level keys named `prod` or `dev` are ordinary user keys. They are not profiles.

A null inside a profile overlay stores null. It does not delete the base key.

Compile-time wiring profiles stay available for sources and secret stores:

```rust
delegate_components! {
    #[unchecked]
    AppLoader {
        preset TwelveFactorPreset,
        #[cfg_wiring(test)]
        SourceLoaderComponent: UseDelegate<TestSources>,
        #[cfg_wiring(feature = "k8s")]
        SecretResolverComponent: UseDelegate<K8sSecrets>,
    }
}
```

`k8s` here is an application feature that selects a satellite resolver. The facade does not grow a `k8s` feature.

### 6.6 Typed deserialization

```rust
#[cgp_component(ConfigDeserializer)]
pub trait CanDeserializeConfig<Config>: HasConfigValueType + HasErrorType {
    fn deserialize_config(&self, value: Self::ConfigValue) -> Result<Config, Self::Error>;
}
```

| Provider | Behavior |
|---|---|
| `SerdeWithProvenance<UnknownKeys>` | `ConfigValue` implements `Deserializer`. Errors carry `Meta`. This is the preset provider |
| `WarnUnknownKeys` | unknown keys become `LoadWarning::UnknownKey` on `HasLoadReport`, with a `strsim` suggestion when the distance is small |
| `DenyUnknownKeys` | the same findings are `ConfigError::Unknown`. Opt-in, also enabled by `#[config(deny_unknown)]` on the root |

`#[serde(default)]` still works. The `Defaults` layer is how those defaults show up in provenance and `explain`: `LoadDerivedDefaults<T>` writes the `ConfigMeta` const table into a `ConfigValue`. Default expressions are `const`. They do not run arbitrary code and they do not read the environment.

Env and CLI strings stay strings in the tree. The deserializer parses a string only when the target type is a bool, number, enum, or sequence (§6.3). A path marked secret uses a deserializer wrapper that drops the raw input from any `serde` error before it is turned into `ConfigError::Type`.

`HasLoadReport` is a getter over a `Vec<LoadWarning>` stored on the loader. `load_config` returns the config. The caller reads the report afterwards. `quick::load` returns `(T, LoadReport)`.

**Typed partial configs** remain available for `no_std` / no-alloc targets, where the dynamic tree is the wrong tool. `#[derive(CgpData)]` plus `#[config(partial)]` exposes `HasOptionalBuilder`. `FinalizeOptionalAll` collects every missing field (capped `heapless::Vec` without alloc, `Vec` with alloc). This path is opt-in. The 0.1 app path is the value tree.

### 6.7 Validation

```rust
#[cgp_component(ConfigValidator)]
#[derive_delegate(UseDelegate<Config>)]
pub trait CanValidateConfig<Config>: HasErrorType {
    fn validate_config(&self, config: &Config) -> Result<(), Self::Error>;
}
```

Providers: `NoValidation`, `ValidateWithGarde` (the struct already derives `garde::Validate`), `ValidateWithValidator`, `ValidateFn<F>`, `AllOf<Product![A, B]>` (runs every validator and aggregates), `Nested` (each section through its own `CanValidateConfig<Section>`).

Validation runs after secret resolution and before publish. Users who want to check a secret's shape are checking the resolved `Secret<T>` via `expose_secret` inside a validator they wrote. The stock validators do not print that borrow.

```rust
ConfigValidatorComponent: UseDelegate<new AppValidators {
    AppConfig: AllOf<Product![Nested, ValidateApp]>,
    HttpConfig: NoValidation,
}>,
```

`ValidationErrors` items are `(KeyPath, code, message, Option<Meta>)`. `code` is a stable `&'static str` (`config::validation`).

The derive does **not** accept `validate(...)` and does **not** forward attributes to garde or validator. The user derives those crates on the struct themselves. Our provider calls `Validate::validate`.

### 6.8 Interpolation

```rust
#[cgp_component(Interpolator)]
pub trait CanInterpolate<Config>: HasConfigValueType + HasErrorType {
    fn interpolate(&self, tree: Self::ConfigValue) -> Result<Self::ConfigValue, Self::Error>;
}
```

`NoInterpolation` is the preset provider. It returns the tree unchanged.

`InterpolateOptedIn` (feature `interpolate`) walks the tree and expands a string only when `ConfigMeta` for `Config` lists that path as `#[config(interpolate)]`. Other strings, including passwords and shell snippets, stay literal.

Grammar on an opted-in string:

| Form | Meaning |
|---|---|
| `$$` | literal `$` |
| `${VAR}` | env snapshot lookup; missing is an error |
| `${VAR:-literal}` | env lookup with a literal default. The default is scanned only up to `max_interpolation_depth` |
| `${VAR:?message}` | env lookup; missing yields `ConfigError::Interpolate` with `message` |
| `${ref:http.host}` | another path in the same tree, after that path is expanded |

Cycles are `ConfigError::InterpolateCycle`. Depth above the limit is `ConfigError::LimitExceeded`. Each expanded node gets `Meta::expanded = true` and is not scanned again, so a value that expands to text containing `${` is stable.

Secret resolution is not an interpolation scheme. There is no `${secret:...}` and no `${file:...}` in this component. File and env secrets go through `CanResolveSecret` (§6.9), which has an allowlist.

### 6.9 Secrets and redaction

`Secret<T>` wraps `secrecy::SecretBox<T>`. `Debug` and `Display` print `[REDACTED]`. `Serialize` is off unless the user opts into a dedicated `serialize_redacted` helper that writes the string `"[REDACTED]"`. `expose_secret()` is explicit. Zeroize runs on drop.

Deserialize accepts:

* a plain string, for tests and for values that are already in a file the operator accepts as sensitive;
* a reference table `{ secret = "env:DB_PASSWORD" }` or `{ secret = "file:db" }` or `{ secret = "docker:db" }`.

The tree stores the reference (and `Meta::secret = true`). It does not store the resolved bytes. `explain` and dumps print `[REDACTED]` for that node even though the reference path itself may be shown; the resolved secret is never in `Meta`.

```rust
#[cgp_component(SecretResolver, mock)]
#[derive_delegate(UseDelegate<Scheme>)]
pub trait CanResolveSecret<Scheme>: HasErrorType {
    fn resolve_secret(&self, reference: &SecretRef) -> Result<SecretString, Self::Error>;
}
```

The async twin `CanResolveSecretAsync<Scheme>` uses `#[cgp_async(Send)]` and is how satellite cloud crates plug in. 0.1 ships sync schemes only.

| Scheme | Provider | 0.1 | Allowlist |
|---|---|---|---|
| `env:` | `EnvSecret` | yes | reads `HasEnvSnapshot` only |
| `file:` | `FileSecret` | yes | path must sit under `HasSecretRoots` |
| `docker:` | `DockerSecret` | yes | `/run/secrets/{name}` only, after the root check |
| `vault:` / `aws:` / `gcp:` | satellite crates | 0.3+ | those crates document their own allowlists |
| (none) | `DenySecrets` | preset default until the `secrets` feature preset is selected | every resolve is `ConfigError::SecretsDenied` |

`HasSecretRoots` defaults to the directory of the loaded config file, `/run/secrets` when that directory exists, and `CREDENTIALS_DIRECTORY` when systemd set it. A reference that escapes the roots (`file:/etc/passwd`, `file:../secret`) is `ConfigError::SecretDenied`. Symlinks are resolved before the check. The miette `SourceCache` stores snippets for non-secret spans only.

Resolution is **eager** in `LoadLayered`: failure at startup is a failed load, and failure during reload keeps the old config. `LazySecret<T>` is an explicit field type. It resolves on first `expose_secret`, using `OnceLock`. Concurrent first calls may resolve twice; both results are equal for a pure resolver, and the loser is dropped (and zeroized). Reload replaces the whole `Arc<Config>`, which drops the previous `LazySecret`. A TTL cache is out of scope.

`Secret<T>: Send + Sync` when `T: Send + Sync`. Resolved secrets are immutable. The reload diff says that a secret path "changed" and omits both values.

Redaction coverage, tested by a grep over fixtures (§10): `Debug`, `Display` of errors, tracing fields, `explain`, `dump`, and the diff. Serde's default "invalid value" text is stripped on secret paths before it becomes `ConfigError`.

### 6.10 Hot reload (0.2)

The traits are specified now so 0.2 does not break 0.1 contexts. The 0.1 facade does not implement the loop.

```rust
#[cgp_component(ConfigReloader)]
#[cgp_async(Send)]
pub trait CanReloadConfig<Config>: HasErrorType {
    async fn reload_config(&self) -> Result<Config, Self::Error>;
}

#[cgp_component(ChangeWatcher)]
#[cgp_async(Send)]
pub trait CanWatchChanges: HasErrorType {
    async fn next_change(&self) -> Result<ChangeEvent, Self::Error>;
}

pub struct ConfigHandle<C: Send + Sync> {
    current: Arc<ArcSwap<C>>,
    notify: async_broadcast::Sender<Arc<C>>,
}
```

`ConfigHandle::load` returns `Arc<C>` (one atomic clone). `subscribe` returns a receiver. On `RecvError::Lagged` the subscriber discards the gap and calls `load()` for the latest `Arc`. There is no callback API; callbacks capture lifetimes that do not survive a swap.

`#[cgp_getter]` methods that return `&Config` belong on a **request context** that holds `Arc<AppConfig>` for the request. The long-lived process context holds `ConfigHandle<AppConfig>` and builds the request context from `handle.load()` at the start of the request. In-flight requests keep their `Arc` after a swap.

`run_reload_loop` is a free function, not a CGP method:

```rust
pub async fn run_reload_loop<Ctx, C>(ctx: Arc<Ctx>)
where
    Ctx: CanWatchChanges + CanReloadConfig<C> + HasConfigStore<C> + CanLog<ReloadFailed> + Sync + Send,
    C: PartialEq + Send + Sync,
{
    // spawned by the application on tokio or smol, not via AsyncRuntime::spawn_task
}
```

`spawn_task` is the wrong launcher: the fork detaches it and discards panics. Each iteration is wrapped in `catch_unwind`. A panic is logged as `ReloadFailed`, the live config stays, and the loop continues. Providers are still written to return `Result` rather than panic; the catch is the backstop.

Steps inside one iteration, using the same pipeline as startup:

1. `next_change` (debounced).
2. Load through `CanReloadConfig` (collect → prepare → interpolate → deserialize → secrets → validate).
3. Build a diff with the derive-generated `ConfigDiff` (requires `PartialEq`, walks `ConfigMeta`, redacts secrets).
4. If any changed path is `#[config(reload = "restart")]`: do **not** swap. Log `ReloadRejected`. Leave the previous `Arc` in place. `ConfigHandle::status()` becomes `RestartRequired { paths }` so a readiness check can report degraded while the process keeps serving the old config. A later successful reload of live fields only, with those restart paths unchanged, returns status to `Live`.
5. If the diff is empty: do **not** notify.
6. Otherwise `ArcSwap::store` the new `Arc` and broadcast it.

A failed load, a failed validate, or a failed secret resolve also leaves the old `Arc` in place.

Watch providers:

| Provider | Behavior |
|---|---|
| `WatchFilesNotify` | notify 8 with a debouncer. Watches parent directories so atomic renames and ConfigMap symlink swaps are visible. Ignores `*~`, `*.swp`, `*.tmp`, and `.#*`. After the debounce, retries a parse up to 3 times, 20 ms apart, so a partial editor write is not a failed reload |
| `WatchResync` | a slow `CanSleep` tick (default 30 s) that runs the same pipeline even when no event arrived. notify can drop events |
| `WatchRemote` | 0.3 satellite sources |
| `WatchSignal` | `SIGHUP`. `#[cfg(unix)]` only. The Windows build uses the resync poll |

`CanTimeout` bounds remote loads once those sources exist. 0.2 file reloads do not need it.

Diffs are generated by the derive from `ConfigMeta` + `PartialEq`, not by reflecting over `serde`. Secret fields contribute a redacted "changed" / "unchanged" bit. Nested sections recurse.

`C: Send + Sync` is required to put the config in the handle. The derive does not add those bounds automatically; the user derives them or the `watch` feature's helper bound fails at the reload context.

### 6.11 Errors and diagnostics

Core (`cgp-config-core`, `alloc` for the owned strings, no `std::io::Error`):

```rust
#[derive(Debug)]
#[non_exhaustive]
#[cgp_auto_error]
pub enum ConfigError {
    #[error("{origin}: {message}")]
    Io { origin: Origin, message: String },
    #[error("{origin}: syntax error: {message}")]
    Parse { origin: Origin, span: Option<Span>, message: String },
    #[error("{path}: {message}")]
    Type { path: KeyPath, message: String, meta: Option<Meta> },
    #[error("missing {path}")]
    Missing { path: KeyPath, expected: String },
    #[error("unknown key {path}")]
    Unknown { path: KeyPath, suggestion: Option<String>, meta: Meta },
    #[error("duplicate key {path}")]
    DuplicateKey { path: KeyPath, meta: Meta },
    #[error("cannot merge {path}: {base_kind} vs {overlay_kind}")]
    MergeConflict { path: KeyPath, base: Origin, overlay: Origin, base_kind: &'static str, overlay_kind: &'static str },
    #[error("validation failed ({count} errors)", count = .0.items.len())]
    Validation(ValidationErrors),
    #[error("interpolation at {path}: {message}")]
    Interpolate { path: KeyPath, message: String },
    #[error("interpolation cycle at {path}")]
    InterpolateCycle { path: KeyPath },
    #[error("ambiguous env key {key}")]
    AmbiguousEnv { key: String, candidates: Vec<KeyPath> },
    #[error("env value for {key} is not valid UTF-8")]
    EnvEncoding { key: String },
    #[error("{origin}: not valid UTF-8")]
    Encoding { origin: Origin },
    #[error("secret reference rejected")]
    SecretDenied { reference: RedactedRef },
    #[error("secrets are disabled")]
    SecretsDenied,
    #[error("migration v{from}→v{to}: {message}")]
    Migration { from: u32, to: u32, message: String },
    #[error("limit exceeded: {what}")]
    LimitExceeded { what: &'static str },
    #[error("reload rejected; restart required")]
    ReloadRejected { paths: Vec<KeyPath> },
}
```

There is no `#[from]` on `Io` or `Parse`. A bare `From<std::io::Error>` would drop the origin. Providers format the std error into `message` at the `std` boundary (`ConfigError::from_io(origin, err)` in `cgp-config-providers`). The core enum stays free of `std::io::Error`, so it can live next to `no_std` + `alloc`.

`ConfigError::code(&self) -> &'static str` returns stable strings (`config::io`, `config::parse`, `config::type`, …). Variants stay `#[non_exhaustive]`.

`Display` of `Type` and `Parse` on a secret path uses a fixed message (`invalid secret value`) and omits the input excerpt. An unknown enum variant uses `ConfigError::Type` and, when a variant is within a small edit distance, puts that name in the message (`did you mean "seconds"`). `RedactedRef` displays the scheme and a hash of the rest, never the raw reference when the reference itself may embed a password (the `file:` and `docker:` forms display the relative name under the allowlist; they do not display file contents).

With the `miette` feature, a std-only `ConfigDiagnostic` wraps `ConfigError` plus an `Arc<str>` `SourceCache` and implements `miette::Diagnostic` with labeled spans. Help text names the env var and the file key ("set APP__HTTP__PORT or add `port` under [http] in config/prod.toml"). Choosing `anyhow::Error` as the context error is supported (`CanRaiseError<ConfigError>` through `From`) and drops labeled spans. Contexts that want spans use `ConfigError` (or `ConfigDiagnostic`) as `HasErrorType::Error`.

`CanRaiseError::raise_error` is `#[track_caller]` in the fork, so eyre reports point at our call sites when the context uses eyre.

Generic library code raises `ConfigError` and adds context with `CanWrapError<KeyPath>` / `CanWrapError<Origin>`.

### 6.12 Schema, templates, documentation (0.2)

```rust
#[cgp_component(SchemaGenerator)]
#[derive_delegate(UseDelegate<Config>)]
pub trait CanGenerateSchema<Config> {
    fn schema(&self) -> ConfigSchema;
}
```

* `SchemarsSchema` (JSON Schema 2020-12) behind feature `schema`.
* `ConfigMeta` from the derive drives templates, Markdown tables, and the env listing (`APP__HTTP__PORT  u16  default 8080  Port to bind`).
* Templates are TOML. Secret fields are rendered as a comment (`# secret: set via secret reference`) with no example value. `const` defaults are rendered.
* `cargo xtask config-docs`, and `config_command()` for clap (`template`, `schema`, `explain`, `check`), which the app attaches under whatever name it wants.
* JSON and RON dumps exist for `explain` output. Comment-preserving rewrite is a TOML promise only (§7).

### 6.13 Library-vs-app usage and multi-crate composition

**Library crate.** Depends on `cgp-config` with `default-features = false, features = ["derive"]`. No `std::fs`, no `std::env`, no loader.

```rust
#[derive(Debug, Clone, PartialEq, serde::Deserialize, cgp_config::ConfigSection)]
#[config(section = "http", getter)]
pub struct HttpConfig {
    /// Port to bind. Under mount `http` and prefix `APP` the variable is `APP__HTTP__PORT`.
    #[config(default = 8080)]
    pub port: u16,
    #[config(default = "0.0.0.0")]
    pub host: String,
    #[config(secret)]
    pub tls_key: Option<Secret<String>>,
}
```

`#[config(getter)]` emits `HasHttpConfig` and `fn http_config(&self) -> &HttpConfig`. A name that already ends in `Config` (`HttpConfig`, and `Http`) both become `HasHttpConfig`. The default derive does not emit the trait.

`#[config(env = "PORT")]` replaces the field segment of that one field. Under mount `http` and prefix `APP` the variable is `APP__HTTP__PORT`. It is not the process-global `PORT`. A true global name is `#[config(env_raw = "PORT")]`, and two `env_raw` values that resolve equal inside one root are a compile error. Do not put `env = "PORT"` on a second field in a section that already has a key `port`: env segments fold case, so the two names are the same key.

Library logic:

```rust
#[cgp_impl(new ServeHttp)]
impl Server where Self: HasHttpConfig + CanLog<String> {
    fn serve(&self) {
        let c = self.http_config();
        let _ = c.port;
    }
}
```

Because `#[cgp_getter]` generates providers only for `UseField` and `WithProvider<FieldGetter>`, the app wires:

* `UseField<Symbol!("http")>` when the context stores `http: HttpConfig` directly;
* `WithProvider<ProjectSection<Path!(@config.http)>>` when the context stores `config: Arc<AppConfig>` (or `&AppConfig`). `Path!` requires the leading `@`. `ProjectSection` walks `HasField` / `MapField` and returns a reference. It does not borrow through `ArcSwap`.

`HttpConfigGetterComponent` is the component name `#[cgp_getter]` emits next to `HasHttpConfig`. The long-lived process will hold `ConfigHandle` (0.2) and copy `Arc<AppConfig>` into the request context at the start of the request.

Optional validator in the library:

```rust
#[cgp_impl(new ValidateHttp)]
impl ConfigValidator<HttpConfig>
where
    Self: CanRaiseError<ConfigError>,
{
    fn validate_config(&self, config: &HttpConfig) -> Result<(), Self::Error> {
        if config.port == 0 {
            return Err(Self::raise_error(ConfigError::validation("http.port", "port is 0")));
        }
        Ok(())
    }
}
```

**App crate:**

```rust
#[derive(Debug, PartialEq, serde::Deserialize, ConfigSection)]
#[config(version = 2)]
pub struct AppConfig {
    #[config(nested, mount = "http")]
    pub http: http_server::HttpConfig,
    #[config(nested, mount = "db")]
    pub db: db_pool::DbConfig,
    pub name: String,
}

#[derive(HasField)]
pub struct Loader {
    app_name: String,
    config_path: PathBuf,
    env_prefix: String,
    active_profile: String,
    env_snapshot: EnvSnapshot,
    file_required: bool,
    load_limits: LoadLimits,
    load_report: LoadReport,
    secret_roots: Vec<PathBuf>,
}

delegate_components! {
    #[unchecked]
    Loader {
        preset TwelveFactorPreset,
        // Spans: use ConfigError as the context error.
        // anyhow is available and drops miette labels (§6.11).
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ConfigValidatorComponent: UseDelegate<new AppValidators {
            AppConfig: AllOf<Product![Nested, ValidateApp]>,
            http_server::HttpConfig: http_server::ValidateHttp,
        }>,
    }
}

config_check! {
    Loader {
        config: AppConfig,
        sources: [Defaults, ConfigFile, ProfileFile, EnvVars],
    }
}

fn main() -> Result<(), ConfigError> {
    let loader = Loader::from_env_and_args("myapp")?;
    let config: AppConfig = loader.load_config()?;
    let _warnings = loader.load_report();
    Ok(())
}
```

Request context once reload exists (0.2):

```rust
#[derive(HasField)]
pub struct Request {
    config: Arc<AppConfig>,
}

delegate_components! {
    Request {
        HttpConfigGetterComponent: WithProvider<ProjectSection<Path!(@config.http)>>,
        DbConfigGetterComponent: WithProvider<ProjectSection<Path!(@config.db)>>,
    }
}
```

The process context stores `ConfigHandle<AppConfig>`, not a bare `Arc` that reload would have to replace by mutation.

**Namespacing**

* Each section type has a default key (`#[config(section = "http")]`).
* The app mounts it with `#[config(mount = "public_http")]`, including several times (`api_http`, `admin_http`).
* Two fields whose resolved mount path is equal are a compile error in the root derive.
* Env names come from `{prefix}__{MOUNT}__{FIELD}` (`APP__HTTP__PORT`). `env = "PORT"` replaces the field segment. `env_raw` bypasses the prefix. Duplicate `env_raw` strings inside one root are a compile error.
* An optional `cgp_namespace!` prefix (`#[prefix(@config.source in DefaultNamespace)]`) keeps component names distinct from other CGP crates.

### 6.14 Top-level loader

```rust
#[cgp_component(ConfigLoader)]
pub trait CanLoadConfig<Config>: HasErrorType {
    fn load_config(&self) -> Result<Config, Self::Error>;
}

pub struct LoadLayered<Sources>(PhantomData<Sources>);

#[cgp_impl(LoadLayered<Sources>)]
impl<Sources, Config> ConfigLoader<Config>
where
    Self: HasEmptyConfigValue
        + CanCollectLayers<Sources>
        + CanInterpolate<Config>
        + CanDeserializeConfig<Config>
        + CanResolveSecrets<Config>
        + CanValidateConfig<Config>,
{
    fn load_config(&self) -> Result<Config, Self::Error> {
        let tree = self.collect_layers(self.empty_config_value(), PhantomData::<Sources>)?;
        let tree = self.interpolate(tree)?;
        let mut config = self.deserialize_config(tree)?;
        self.resolve_secrets(&mut config)?;
        self.validate_config(&config)?;
        Ok(config)
    }
}
```

`CanCollectLayers` is where file migration and `$profiles` expansion run, via `CanPrepareLayer`. They are not a second, conflicting stage after interpolation.

`CanResolveSecrets<Config>` walks `ConfigMeta` for `#[config(secret)]` fields and `Secret<T>` fields, resolves references, and writes `Secret<T>`. `DenySecrets` returns `SecretsDenied` if any secret field is present and unresolved, and succeeds when the config has no secret fields.

The async twin `LoadLayeredAsync<Sources>` (§8) uses the same later stages. Only the collect step differs.

### 6.15 Presets

```rust
cgp_preset! {
    #[version(1)]
    CoreConfigPreset {
        type ConfigValue = cgp_config::Value,
        EmptyConfigValueComponent: EmptyMap,
        LayerMergerComponent: DeepMerge,
        LayerCollectorComponent: CollectInOrder,
        LayerPreparerComponent: UseDelegate<DefaultPreparers>, // file markers → PrepareFileLayer, others → IdentityLayer
        InterpolatorComponent: NoInterpolation,
        ConfigDeserializerComponent: SerdeWithProvenance<WarnUnknownKeys>,
        FormatParserComponent: UseDelegate<EnabledFormats>,
        ConfigValidatorComponent: NoValidation,
        SecretResolverComponent: DenySecrets,
        LoadLimitsComponent: DefaultLoadLimits,
        LoadReportComponent: ReportOnLoader,
    }
}

cgp_preset! {
    #[version(1)]
    TwelveFactorPreset: CoreConfigPreset {
        override SourceLoaderComponent: UseDelegate<TwelveFactorSources>,
        override ConfigLoaderComponent:
            LoadLayered<Product![Defaults, ConfigFile, ProfileFile, EnvVars]>,
    }
}
```

`DefaultPreparers` lists every built-in marker. A marker without a preparer is a compile error, which is what we want.

`TwelveFactorSources` wires `Defaults`, `ConfigFile`, `ProfileFile`, and `EnvVars` only. `ProfileFile` is optional when the file is absent. `ConfigFile` is required once bootstrap has chosen a path.

Feature presets replace the source list and the loader with `override`. They are the supported way to add layers, because a child preset cannot splice one entry into a `Product!`:

```rust
cgp_preset! {
    #[version(1)]
    TwelveFactorDotenv: TwelveFactorPreset {
        override SourceLoaderComponent: UseDelegate<TwelveFactorDotenvSources>,
        override ConfigLoaderComponent:
            LoadLayered<Product![Defaults, ConfigFile, ProfileFile, DotEnv, EnvVars]>,
    }
}

cgp_preset! {
    #[version(1)]
    TwelveFactorCli: TwelveFactorPreset {
        override SourceLoaderComponent: UseDelegate<TwelveFactorCliSources>,
        override ConfigLoaderComponent:
            LoadLayered<Product![Defaults, ConfigFile, ProfileFile, EnvVars, CliArgs]>,
    }
}

cgp_preset! {
    #[version(1)]
    TwelveFactorDotenvCli: TwelveFactorPreset {
        override SourceLoaderComponent: UseDelegate<TwelveFactorDotenvCliSources>,
        override ConfigLoaderComponent:
            LoadLayered<Product![Defaults, ConfigFile, ProfileFile, DotEnv, EnvVars, CliArgs]>,
    }
}
```

`TwelveFactorDotenv` is compiled only with feature `dotenv`. `TwelveFactorCli` and `TwelveFactorDotenvCli` require `clap` (and `dotenv` for the last). The default build does not mention those providers, so it compiles with default features.

```rust
cgp_preset! {
    #[version(1)]
    TwelveFactorLocal: TwelveFactorPreset {
        override SourceLoaderComponent: UseDelegate<TwelveFactorLocalSources>,
        override ConfigLoaderComponent:
            LoadLayered<Product![Defaults, ConfigFile, ProfileFile, LocalFile, EnvVars]>,
    }
}
```

`LocalFile` is optional when `{stem}.local.{ext}` is absent. It stays out of `TwelveFactorPreset` so a production host does not silently pick up a developer file. Dev and test contexts use `TwelveFactorLocal`.

```rust
cgp_preset! {
    #[version(1)]
    SecretsPreset: TwelveFactorPreset {
        override SecretResolverComponent: UseDelegate<StdSecretSchemes>, // env:, file:, docker:
    }
}

cgp_preset! {
    #[version(1)]
    InterpolatingPreset: TwelveFactorPreset {
        override InterpolatorComponent: InterpolateOptedIn,
    }
}

cgp_preset! {
    #[version(2)]
    TestConfigPreset: CoreConfigPreset {
        override SourceLoaderComponent: UseDelegate<TestSources>, // InMemory + EnvSnapshot
        override ConfigLoaderComponent: LoadLayered<TestLayers>,  // Defaults, InMemory, EnvVars
        LoggerComponent: NoopLogger,
        // P4: override SecretResolverComponent: UseDelegate<MockSecretSchemes>,
    }
}
```

`EmbeddedPreset` (no_std) wires typed partial builders and a postcard or flash source. It does not wire `ConfigValue`.

`StrictPreset` extends `TwelveFactorPreset` and overrides the deserializer to `SerdeWithProvenance<DenyUnknownKeys>` and the source loader to `UseDelegate<StrictSources>`, which is `StdSources` with `ProfileFile: RequireActiveProfile` (a missing active profile is an error). A preparer cannot do this: it only sees layers that exist, never the missing profile file. Replacing the whole source loader with a single provider would drop the other markers. Unknown-key denial is not the default.

**Behavior changes ship as new preset types** (`TwelveFactorV2`), not as a bump of `#[version]` on the same type. `#[version]` stays the fork's module `VERSION` and the `override` / `PresetContains` check. Rustdoc on each preset states the layer list, the unknown-key policy, and the interpolation policy.

`config_check!` is part of every preset's example.

### 6.16 `config_check!`

```rust
config_check! {
    Loader {
        config: AppConfig,
        sources: [Defaults, ConfigFile, ProfileFile, EnvVars],
    }
}
```

expands to `check_components!` with real parameters for `ConfigLoaderComponent`, `SourceLoaderComponent`, `ConfigValidatorComponent`, `LayerPreparerComponent`, and `SecretResolverComponent`. It is the supported alternative to reading a `()` wiring error. Contexts still mark the table `#[unchecked]` because the generated `__CanUse` check cannot name those parameters (§1.3).

### 6.17 Capabilities a Rust app actually calls

These are part of the crate, not samples left to each app.

**Load a struct without a process.** Unit tests and `include_str!` defaults need an entry point that does not open a file and does not read `std::env`:

```rust
let cfg: AppConfig = cgp_config::from_toml_str(include_str!("../tests/app.toml"))?;
let cfg: AppConfig = cgp_config::from_json_str(json)?;
let cfg: AppConfig = AppConfig::from_defaults()?;
```

`from_toml_str` and `from_json_str` run the same deserialize, secret, and validate stages as the loader, with `DenySecrets` and an empty env snapshot. `from_toml_str_with` and `from_json_str_with` take the secret resolver and return `Loaded { config, warnings }`; unknown keys in a `deny_unknown` section and missing required fields are reported in one aggregated error. `from_defaults` deserializes only `ConfigMeta` const defaults. Doctests use these functions. `LoadEmbedded<Toml>` is the pipeline form of the same text: a source marker in front of `ConfigFile`, fed by `HasEmbeddedDocument`.

**`{stem}.local.{ext}`** (`app.local.toml`). `TwelveFactorLocal` (§6.15). The file is gitignored by convention. The book says so. Production presets do not load it.

**One JSON document in the environment.** Containers sometimes inject the whole config as a single variable. `{PREFIX}_DOCUMENT` (prefix `APP` → `APP_DOCUMENT`) is parsed as JSON when the `json` feature is on and the variable is present. It is the `EnvDocument` layer. The user inserts it where they want it in the `Product!` list, almost always after files and before `EnvVars`, so individual `APP__...` keys still win. Unset means `Ok(None)`. `APP_CONFIG` stays a filesystem path. Bootstrap removes `APP_CONFIG`, `APP_PROFILE`, and `APP_DOCUMENT` from the snapshot before the env layer runs.

**Durations and sizes.** A small parser in `cgp-config-value`, no `humantime` dependency:

| Attribute | Input | Output |
|---|---|---|
| `#[config(parse = "duration")]` | `30s`, `5m`, `1h`, `100ms`, `1s200ms` | `DurationParts { secs, nanos }` in core, converted to `std::time::Duration` behind `std` |
| `#[config(parse = "bytes")]` | `10MiB`, `1KB`, `512` | `u64` bytes. A bare number is bytes. `KB` is 1000, `KiB` is 1024 |

A duration without a unit is `ConfigError::Type`. `PathBuf`, `IpAddr`, and `SocketAddr` use `FromStr` and need no attribute.

`**#[config(flatten)]`.** A section field's children appear on the parent in `ConfigMeta`, including env names. A flattened `HttpConfig` with field `port` under prefix `APP` produces `APP__PORT`, not `APP__HTTP__PORT`. A name clash with a sibling field (or an alias, or an env name after folding) is a compile error, from the `const` check `ConfigMeta::checked` the derive emits. A key written both on the parent and under the flattened field in one document is `DuplicateKey` at load time.

**Unknown keys per section.** `#[config(deny_unknown)]` on a section struct denies unknown keys in that subtree even when the app's global policy is `WarnUnknownKeys`. The root attribute denies the whole document. Libraries ship the stricter policy on their own struct when an unknown key would be a typo for a required setting.

`**$include` (0.2).** Reserved in 0.1 so a user field cannot take the name. In 0.2, a root key `$include = ["db.toml", "net.toml"]` loads those paths relative to the including file, in order. Each file is prepared with the same version and profile rules, then deep-merged at the root in place of `$include`. Paths stay inside the including file's directory after symlink resolution. `..`, absolute paths, and a cycle are `ConfigError::IncludeDenied`. Depth uses `LoadLimits`. Globs are not part of 0.2.

**Decrypt wrapper (extension point, 0.3 satellite).** `ReadDecoded<Inner, Decoder>` sits in front of `CanReadConfigFile`. The default reader does not require a decoder component, so 0.1 contexts do not grow a new bound. An age or sops crate replaces the reader with that wrapper. Ciphertext is decoded before parse. The decoder's keys follow the same root allowlist idea as file secrets.

**Fingerprint.** `LoadReport::fingerprint()` is the FNV-1a 64 of the canonical redacted JSON (secrets replaced with `"[REDACTED]"`, map keys sorted). It is stable across Rust releases because it does not use `std` `DefaultHasher`. The startup log event includes this hex and the winning origin. It is an identifier for support, not a MAC.

**Subprocess env (0.2).** `to_env_pairs(&config) -> Vec<(String, String)>` uses `ConfigMeta` names and skips secret fields. An app that spawns a child uses this instead of copying its own process environment.

---

## 7. Config file versioning and migrations (0.2)

One mechanism. The derive takes a chain type. 0.1 records `$version` and removes it before deserialize (§6.5). 0.2 is what runs the chain inside `PrepareFileLayer`, before `$profiles` expansion, so a migration still sees the tables the author wrote.

```rust
#[config(version = 2, migrate = migrations::AppMigrations)]
```

`AppMigrations` is `MigrationChain<Product![V1ToV2]>`. There is no parallel `migrate = path::fn` string. `CanMigrate<Config>` has `#[derive_delegate(UseDelegate<Config>)]`. `NoMigration` is not required on the 0.1 loader because file preparation in 0.1 treats `$version` as data and leaves it for 0.2; 0.1 documents `$version` and ignores it except to strip it before deserialize so it does not become an unknown key. 0.2 wires the chain into `PrepareFileLayer`.

Reserved key: `**$version**` (integer). A user field cannot be named `$version`; the derive rejects it. `$profiles` and `$include` are reserved the same way. `$include` is loaded in 0.2 (§6.17). In 0.1 the key is rejected at derive time and, if a hand-written file contains it, stripped into `LoadWarning::IgnoredInclude` so it does not become a user field.

Rules:

* Absent `$version` means version 1. `HasLoadReport` records `AssumedVersion`. Templates, `dump`, and `migrate --write` always emit the current `$version`. A hand-written file for schema N > 1 includes `$version` or it will be migrated from 1.
* Each step is `fn(ConfigValue) -> Result<ConfigValue, ConfigError>`: rename, move, split, or rescale. Steps run in order. Nodes record `Meta::migrated_from`.
* When the document version equals the schema version, the chain is a no-op (idempotent).
* Aliases (`#[config(alias = "listen_port", deprecated = "use http.port")]`) accept the old name from any layer, including env, and record `LoadWarning::DeprecatedKey`. Aliases are how env keeps working across a rename. Migrations are how files change shape.
* `migrate --write` renders TOML with `toml_edit`, preserving comments. It writes a temporary file in the same directory, `fsync`s, and renames over the destination. It does not truncate the destination in place. The `$version` stamp is part of the temp file's contents, so a crash before rename leaves the previous file and the previous version. A second run sees the current version and writes nothing.
* JSON and RON rewrites are lossy of comments and are behind an explicit `--format` flag. YAML comment preservation, when YAML exists, is best-effort and documented as such. The default `--write` format is TOML.
* Golden test: every historical fixture loads into the current struct.

Profile expansion is part of 0.1, because `{stem}.{profile}.{ext}` and `$profiles` are how 0.1 selects an environment. Migration steps are 0.2. In 0.1, `PrepareFileLayer` expands `$profiles`, records `$version` on `HasLoadReport`, and removes that key without rewriting any other key. Key rewrites wait for the chain.

---

## 8. Async support

Sync is the 0.1 path. Async traits use `#[cgp_async(Send)]` so a later runtime feature does not change the trait.

`LoadLayeredAsync` collects by:

1. Building the type-level list into a compile-time sequence of load futures (a small fold, not `futures::join!`, which needs a fixed tuple).
2. Awaiting them concurrently.
3. Merging the `Result`s **in list order** so precedence stays deterministic.
4. Running prepare on each `Some` layer in that same order.
5. Running interpolate, deserialize, resolve, and validate exactly as the sync loader does.

A missing optional source is `None`. A parse or IO error fails the load. There is no partial success.

Sync file and env providers called from the async loader run through the runtime's `spawn_blocking` equivalent (`tokio::task::spawn_blocking`, or smol's blocking pool). They do not run `std::fs` on the async executor thread. The adapter is `BlockingSource<P>` and it is wired automatically by `LoadLayeredAsync` for markers whose provider is sync.

Timeouts use `CanTimeout` (`TimeoutWithRuntime` maps `TimeoutElapsed` through `CanRaiseError<ConfigError>`). Retries are `RetryWithBackoff<P>` on `CanSleep`, used by remote sources in 0.3. 0.1 does not retry a local file.

Single-threaded contexts that need `?Send` futures use a separate trait `CanLoadSourceLocalAsync` with `#[cgp_async]` (no `Send`). The preset async loader is the `Send` one. We do not pretend one trait covers both.

---

## 9. `no_std` / minimal configurations

| Tier | Crates | Gives |
|---|---|---|
| `core` (no alloc) | `cgp-config-core` + `cgp-fork` with `default-features = false` | component traits, `LoadLimits`, `KeyPath` of `&'static str` for typed builders, `FinalizeOptionalAll` into `heapless::Vec` (cap 32) |
| `alloc` | + `cgp-config-value` | `ConfigValue`, `Meta`, fallible merge, interpolation, serde `Deserializer`, `ConfigError` owned strings, dynamic `KeyPath` |
| `std` | + providers, formats, secrets | files, env snapshot, dotenv, clap |

Dynamic key paths from files need `alloc`. The no-alloc tier is the typed partial-builder path only.

No `std::env` and no `std::fs` in `core` or `value`. Environment access goes through `HasEnvSnapshot`. File access goes through `CanReadConfigFile` in the std providers.

`Meta` is always present on alloc nodes. There is no `provenance` feature that changes the tree type under feature unification. A span may be `None` when the parser has no spans.

---

## 10. Testing strategy

* **Unit tests per provider**, against `TestConfigPreset`. Env comes from `EnvSnapshot`. Files come from `ReadFromMap` or a unique `tempfile` directory per test. Tests do not change the process current directory and do not call `set_var`. They stay parallel under `cargo test`. `from_toml_str`, `from_json_str`, and `from_defaults` have their own tests, including a duration, a byte size, a map keyed from env, and a world-writable Unix file that warns.
* **Mocks:** `mock` generates `MockSecretResolver` and `MockSourceLoader`. Tests assert call counts and queued returns.
* **Wiring tests:** `config_check!` for every preset and every example context. trybuild UI tests cover wiring mistakes and derive misuse, pinned to a toolchain the way the fork pins `tests/ui`.
* **Golden diagnostics:** insta snapshots of `Display` and, with `miette`, of the rendered diagnostic, for each error `code`.
* **Redaction:** a grep-style test over Debug, error Display, tracing captures, explain, dump, and diff. Fixtures include a secret that would be echoed by serde's default error. The substring must be absent.
* **Property tests (proptest):** `DeepMerge` associativity and empty-layer identity; later scalar wins; type conflicts error; env round-trip through the type-directed deserializer (`tree of strings → struct → env text → struct`); interpolation idempotence on already-expanded nodes; interpolation leaves non-opted-in strings unchanged, including strings that contain `${`.
* **Format conformance (0.1):** the same fixture in TOML and JSON deserializes to the same struct, including spans where the format has them. Duplicate keys fail in both.
* **Fuzzing (cargo-fuzz):** each parser wrapper, the interpolation lexer, env-key splitting, and derive input. The derive uses the same depth and identifier limits as the fork's macro-impl crate.
* **Reload tests (0.2):** tokio `start_paused` plus the debounce window; a partial write that becomes valid on the retry; atomic rename; symlink swap; a failed validation keeps the old `Arc`; an empty diff does not notify; a `restart` field change does not swap; a lagged subscriber observes the latest `Arc`. Unix-only test for `SIGHUP`. Windows CI runs the resync path.
* **Feature matrix:** on pull requests, `cargo hack --each-feature` for the facade and `--feature-powerset` for `cgp-config-core` and `cgp-config-value` only. A full powerset of the facade's feature list is nightly, not per PR.
* **Miri** on `cgp-config-value` is nightly CI.
* **MSRV job:** 1.99.
* `**no_std` build** of core on `thumbv7em-none-eabihf`.
* Doctests that need data use `InMemory` / `EnvSnapshot`. They do not read the developer's disk or environment.

Line-coverage gates apply to `cgp-config-value` and `cgp-config-core` (target 95% on value). The workspace as a whole has no percentage gate; proc-macro crates and feature-gated providers make that number noise.

---

## 11. Performance

* The static pipeline compiles to direct calls. Hot paths are parse, merge, and deserialize.
* Nodes are `Arc`. Keys are `Arc<str>`. Maps are `IndexMap` with the `alloc` feature, so order is stable for diffs and templates. `Meta` lives in a side arena indexed by node id. Unchanged subtrees are shared across layers, so a 6-layer merge does not clone the whole tree each time.
* The provenance chain is capped (§6.1). Spans are always part of the type; they are `None` when unknown.
* Deserialization borrows the tree (`Deserializer` for `&ConfigValue`). There is no JSON round-trip and no `Serialize` bound on the user's config.
* After load, readers hold `Arc<Config>`. Under reload, `ArcSwap::load` is wait-free. A request keeps the `Arc` it loaded at the start.
* **Compile time.** CGP generics and type-level lists can slow builds. 0.2 adds a compile-time bench (a flat 200-component table and a 64-field record, the shape the fork's rewrite plan §10 uses). The gate compares the **median of 5 runs on one runner class** and fails at >25%. A single sample is not a gate. `Symbol!` keys stay short. Our traits carry `#[diagnostic::on_unimplemented]` messages (fork ADR 0005 style).
* Criterion benches (0.2, baselines recorded, median regression >10% fails the PR): 1k-key TOML load, 6-layer merge, 10k env vars, reload latency, `explain`.

---

## 12. Observability

* Stages log through `CanLog<Detail>` with typed details: `SourceLoaded { origin, key_count }`, `LayerSkipped { origin }`, `ReloadApplied { changed_paths }`, `ReloadRejected { paths }`, `ReloadFailed { code }`, `DeprecatedKey { path }`. Details contain keys and origins. They do not contain values.
* Contexts wire `TracingLogger` (`log-tracing`) or `NoopLogger` (the default in `CoreConfigPreset`, so a loader compiles without a subscriber).
* Tracing events use the same fields. Metrics systems scrape those events. 0.1 does not grow a `CanRecordMetric` component.
* `explain(path)` reports the winning origin and span, the shadowed origins, and `[REDACTED]` when `Meta::secret` is set or the path is a `Secret<T>`.
* The startup `CanLog` detail `ConfigLoaded { origin, fingerprint }` carries `LoadReport::fingerprint()` (§6.17) and no values.
* `dump()` writes the effective config with secrets redacted and with origin comments in TOML.
* The `inspect` feature forwards the fork's `inspect`, so `cargo xtask wiring` can draw a loader context.

---

## 13. API ergonomics

* One import for apps: `use cgp_config::prelude::*;`, which re-exports the `cgp_fork::prelude` items this crate needs.
* Three levels:
  1. **Quick:** `cgp_config::quick::load::<AppConfig>("myapp") -> Result<(AppConfig, LoadReport), ConfigError>`. Builds a loader with `TwelveFactorPreset`, takes one `EnvSnapshot` of the process environment, and follows the search order in §6.3. App-only. Documented as reading the process environment. Libraries do not call it. Doctests call `quick::load_from(snapshot, files)` instead.
  2. **Standard:** a loader context, `preset TwelveFactorPreset`, `config_check!`, and explicit entries that replace preset ones (`DenyUnknownKeys`, a validator table, `SecretsPreset`).
  3. **Advanced:** custom providers, `LoadLayeredAsync`, typed partial builders, satellite secret crates.
* `StdLoader::builder().app_name("x").file("cfg.toml").env_prefix("APP").profile("prod")` fills the loader. The prefix is stored without a trailing separator (`APP`, not `APP_` or `APP__`); a prefix that is empty or ends with `_` is rejected. The builder does not read the process environment unless `capture_env()` is called. Its signature does not expose CGP types.
* Provider names read clearly in a wiring table: `LayerMergerComponent: DeepMerge`.

`quick::load` search order is exactly §6.3. The app name is the `{app}` segment and the default prefix (`MYAPP` for `"myapp"`, so nested keys look like `MYAPP__HTTP__PORT`). Profile defaults to `default`.

---

## 14. Derive macro: `#[derive(ConfigSection)]`

**Struct attributes:** `section = "key"`, `version = N`, `migrate = ChainType` (0.2), `env_prefix`, `deny_unknown`, `rename_all`.

**Field attributes:** `default = <const expr>`, `env = "SUFFIX"`, `env_raw = "EXACT"`, `secret`, `alias = "old"`, `deprecated = "msg"`, `reload = "live" | "restart"`, `merge = "replace" | "append" | "deep"`, `interpolate`, `parse = "duration" | "bytes"`, `flatten`, `mount = "path"` (on a section field), `nested`, `skip`.

There is no `validate(...)` passthrough.

**Emits always:** `ConfigMeta` (keys, docs from `///`, const defaults, env names, secret flags, aliases, reload policy, interpolate flags, merge policy), the data `LoadDerivedDefaults` reads, and `PartialEq`-based diff helper stubs used when the user also derives `PartialEq`.

**Emits only when asked:**

* `#[config(getter)]` — the `Has{Name}Config` `#[cgp_getter]` trait.
* `#[config(partial)]` — `CgpData` optional builders and `FinalizeOptionalAll`.

The default derive does not emit `HasField` or CGP wiring. A `quick::load` user pays for `ConfigMeta` and serde, not for a CGP context.

**Reserved field names:** `$version`, `$profiles`, `$include`. The derive errors if the user declares them.

**Compile errors, all reported together** (spans, no early abort): duplicate mounts, duplicate `env_raw`, `env` suffix collisions under one prefix, unknown attributes, non-const defaults, nesting deeper than the fork's macro limit, identifiers over the length limit.

Implementation: `parse → validate → emit` in `cgp-config-derive-impl`, same structure as the fork's ADR 0004, fuzzed.

---

## 15. Feature flag matrix (facade `cgp-config`)

| Feature | Default | Implies | Enables |
|---|---|---|---|
| `std` | yes | `alloc` | file, env snapshot, platform dirs |
| `alloc` | via `std` | — | value tree, merge, `ConfigError` strings |
| `derive` | yes | — | `#[derive(ConfigSection)]` |
| `toml` | yes | `alloc` | TOML parse and render, with spans |
| `json` | no | `alloc` | JSON parse and render, duplicate keys rejected |
| `json5` | no | `alloc` | JSON5 (after 0.1) |
| `ron` | no | `alloc` | RON (after 0.1) |
| `ini` | no | `alloc` | INI (after 0.1) |
| `yaml` | no | `alloc` | YAML, alias expansion disabled (after 0.1) |
| `dotenv` | no | `std` | `LoadDotenv`, `TwelveFactorDotenv` |
| `clap` | no | `std` | `HasCliOverrides` adapter, `config_command()` |
| `interpolate` | no | `alloc` | `InterpolateOptedIn` |
| `secrets` | no | `alloc`, `std` | `Secret<T>`, `env` / `file` / `docker` resolvers, `SecretsPreset` |
| `validate-garde` | no | — | `ValidateWithGarde` |
| `validate-validator` | no | — | `ValidateWithValidator` |
| `async` | no | `std` | async traits' std helpers, `BlockingSource` |
| `tokio` | no | `async`, `cgp-fork/tokio` | tokio runtime adapter |
| `smol` | no | `async`, `cgp-fork/smol` | smol runtime adapter |
| `watch` | no | `async` | notify 8, `ConfigHandle`, `run_reload_loop` (0.2) |
| `schema` | no | `derive` | schemars (0.2) |
| `templates` | no | `derive`, `toml` | TOML templates and Markdown (0.2) |
| `miette` | no | `std` | `ConfigDiagnostic` |
| `anyhow` / `eyre` / `std-error` | no | the matching `cgp-fork` feature | error backends |
| `log-tracing` | no | `cgp-fork/log-tracing` | `TracingLogger` |
| `mock` | no | `cgp-fork/mock` | generated mocks |
| `inspect` | no | `cgp-fork/inspect` | wiring graph |

`default = ["std", "derive", "toml"]`.

There is no `provenance` feature. `Meta` is part of the alloc tree.

There is no `secrets-vault`, `secrets-aws`, `secrets-gcp`, `remote-http`, `remote-consul`, or `remote-etcd` feature on the facade. Those are satellite crates (§5) so the facade's lockfile and feature resolver stay free of those SDKs.

Features are additive. `cargo hack` builds every feature (§10, §17). `docs.rs` uses `all-features` with `doc_cfg`. Default features do not change the `ConfigValue` type; turning `secrets` or `interpolate` on adds providers and types and does not rewrite the tree.

---

## 16. MSRV, semver, and releases

* **MSRV 1.99** (`rust-version` and `rust-toolchain.toml`), checked in CI. Above cgp-fork's 1.89 floor. An MSRV bump is a minor release, announced one minor ahead.
* 0.1 ships at the end of P5 (§18). Pre-1.0 continues until two real apps and two libraries have used the 0.1 surface. 1.0 requires that surface to be stable. Adding a component or a preset after 1.0 is non-breaking. Changing `TwelveFactorPreset`'s layer list is breaking and is done by publishing `TwelveFactorV2` while leaving `TwelveFactorPreset` in place until a major bump.
* **Semver surface:** consumer traits, provider names, preset names, feature names, `ConfigError` variants and `code()` strings, `ConfigMeta` attribute syntax, the reserved keys `$version` and `$profiles`, and the public crates `cgp-config`, `cgp-config-core`, `cgp-config-value`, `cgp-config-providers`, `cgp-config-formats`, `cgp-config-secrets`, `cgp-config-derive`. `cgp-config-derive-impl` and the value-arena internals have no stability promise.
* **Fork dependency:** caret requirement `cgp-fork = "1.1"`. A major bump of the fork is a major bump for us, because CGP types appear in our public traits. A thin `cgp_config::cgp` re-export is the only path through which our public signatures name the fork, so a move to upstream `cgp` replaces that module. Track the fork's `UPSTREAM_COMMIT`. Weekly CI builds against the fork's `main`.
* **License:** `AGPL-3.0-or-later` on every published crate (`[workspace.package] license` + root `LICENSE`). The fork is Apache-2.0; that is compatible as a dependency.
* Tooling: `cargo-semver-checks`; `release-plz`; `cargo-deny` with the license allowlist MIT, Apache-2.0, Unicode-3.0, Zlib, BSD-3-Clause (dependency licenses; our own SPDX is AGPL-3.0-or-later); a Keep a Changelog `CHANGELOG`.

---

## 17. CI, docs, benchmarking

**Pull-request CI** (`cargo xtask ci`, same command locally and in GitHub Actions):

* fmt (nightly) and clippy `-D warnings` with the fork's pedantic set.
* Tests on Linux, macOS, and Windows.
* `cargo hack --each-feature` on the facade.
* Feature powerset on `cgp-config-core` and `cgp-config-value`.
* MSRV 1.99.
* `no_std` target build of core.
* `cargo-deny`, `cargo-audit`.
* `cargo xtask semver` (`cargo-semver-checks` against `SEMVER_BASELINE`, default `origin/main`). Advisory until 0.1.0. The baseline is that revision, not a tag invented for the alpha. The surface it covers is the P1–P5 public API in §16.
* trybuild UI tests pinned to a toolchain.
* Doc build with `-D rustdoc::broken_intra_doc_links`.
* Coverage gate on value and core only.

**Nightly CI:** Miri on `cgp-config-value`; fuzz smoke extended to long runs; facade feature powerset; testcontainers for satellite crates when those crates exist (0.3).

**0.2 adds:** the compile-time median gate (§11) and the criterion regression gate.

**Docs (the 0.1 surface):** the mdBook in `book/` (guide: quick start, library authors, app authors, 12-factor, secrets, `no_std`; cookbook: CLI tools, multi-crate workspaces, docker secrets, coming from config-rs / figment / confique; reference: components, providers, presets, attributes, the pipeline, the semver surface; one page per `ConfigError::code()`). Reload, migrations, and templates are added to the book in the release that ships them. Runnable snippets are crate doctests (`quick::load_from`, `ProjectSection`) and `examples/multi-crate-workspace`. The coming-from page states the env separator `__`, the warning policy for unknown keys, and the fact that we do not mutate the process environment. `cargo xtask book` builds the book when `mdbook` is installed; CI does not require it.

---

## 18. Phased roadmap

| Phase | Release | Est. | Deliverables | Acceptance |
|---|---|---|---|---|
| **P0 Spike** | — | done in part | Prototype listed in Appendix A. Findings §1.3 written up as ADR-001 | The ADR records where this plan supersedes the prototype |
| **P1 Core** | 0.1 | 3 wk | `cgp-config-core`, `cgp-config-value` (`Arc` nodes, `Meta`, fallible merge, `Deserializer`), `ConfigError`, `KeyPath`, `LoadLimits`, `CoreConfigPreset`, `HasEmptyConfigValue` | `no_std` and `alloc` builds; merge property tests; 95% line coverage on value; Miri clean on nightly |
| **P2 Sources and formats** | 0.1 | 3 wk | defaults from `ConfigMeta`, embedded document, file / search (cwd, exe dir, `CONFIGURATION_DIRECTORY`, platform dirs) / `conf.d`, profile file, `$profiles`, optional local file, env snapshot, `{PREFIX}_DOCUMENT`, dotenv feature, clap feature, TOML and JSON with spans and duplicate-key rejection, torn-read retry, Unix mode warnings, `TwelveFactorPreset` and the dotenv/cli/local child presets | Format-conformance suite green for TOML and JSON; errors show file:line:col; hermetic env tests; default features compile without dotenv or clap; `CONFIGURATION_DIRECTORY` wins over cwd when set |
| **P3 Derive, validation, interpolation** | 0.1 | 3 wk | `#[derive(ConfigSection)]`, const defaults, `from_toml_str` / `from_defaults`, duration and byte parsers, `flatten`, per-section `deny_unknown`, garde/validator adapters (no attribute proxy), `AllOf` / `Nested`, `InterpolateOptedIn`, `WarnUnknownKeys` / `DenyUnknownKeys` | Aggregated errors; trybuild for derive misuse; fuzz on the derive and the lexer; non-opted-in `${` strings survive; `30` is rejected as a duration and `30s` parses |
| **P4 Secrets and diagnostics** | 0.1 | 2 wk | `Secret<T>`, env/file/docker resolvers, `HasSecretRoots` including `CREDENTIALS_DIRECTORY`, zeroizing raw buffers, redaction, fingerprint, `miette` feature, `explain` | Redaction grep covers Debug, Display, logs, explain, dump; a group-readable file that contains a secret reference warns |
| **P5 Libraries, quick load, 0.1 book** | **0.1** | 3 wk | getter convention, mounts, `ProjectSection` or the documented explicit provider, `examples/multi-crate-workspace`, `quick::load`, book for this surface, semver-checks baseline | A library builds against the facade with `default-features = false, features = ["derive"]` and links no std IO; the app wires two sections; duplicate mounts and duplicate `env_raw` fail the derive; doctests pass |
| **P6 Watch and reload** | 0.2 | 3 wk | `LoadLayeredAsync`, notify 8, scratch-file ignore, resync poll, Unix `SIGHUP`, `ConfigHandle`, `status()`, `run_reload_loop`, diff, restart-required rejection | Paused-time tests on 3 OSes; failed reload keeps the old `Arc`; empty diff does not notify; restart field does not swap and status is `RestartRequired`; Windows omits `SIGHUP` and passes resync; symlink-swap test |
| **P7 Migrations, schema, templates** | 0.2 | 2 wk | `MigrationChain`, crash-safe `migrate --write`, root `$include`, `to_env_pairs`, schemars, TOML templates, Markdown, env listing | Template round-trips to defaults; v1→vN fixtures load; second `--write` is a no-op; kill -9 during write leaves the previous file (fault-injection test); an `$include` that escapes the directory is denied |
| **P8 Satellites** | 0.3 | ongoing | HTTP, Consul, etcd, Vault, AWS, GCP, and an age/sops reader wrapper, as separate crates implementing the 0.1 traits | Mocked unit tests in those crates' PR CI; testcontainers on nightly |
| **P9 1.0** | 1.0 | ongoing | API freeze of the 0.1/0.2 surface after two apps and two libraries, long fuzz, embedded example | No open P1 bugs; 1.0 RFC accepted |

P5's shipped wiring is recorded in ADR-004: `Path!(@config.http)` inside `WithProvider<ProjectSection<...>>`, `#[config(getter)]` and `#[config(partial)]`, `cgp_config::quick::load` / `load_from`, `StdLoader::builder()` with a prefix that has no trailing `_`, and the 0.1 book in `book/`. The worked example is `examples/multi-crate-workspace`.

Extra formats (YAML, JSON5, RON, INI) land as features whenever their parser is ready. They are not on the 0.1 critical path. YAML ships only with alias expansion disabled.

---

## 19. Decisions, risks

### Decisions (closed)

| Topic | Decision |
|---|---|
| Value tree | Our `ConfigValue` with `Arc` nodes and a `Meta` arena. Figment and config-rs bridges are a post-0.1 crate |
| Env coercion | Strings in the tree. The deserializer parses bools, numbers, enums, sequences, `PathBuf`, `IpAddr`, and `SocketAddr`. Durations require a unit. Byte sizes accept a bare number as bytes. No comma splitting |
| Maps in env | One path segment is the map key, lowercased. File keys keep their case |
| Local file | `{stem}.local.{ext}` only through `TwelveFactorLocal`, never the production preset |
| Embedded and tests | `from_toml_str`, `from_json_str`, `from_defaults`, and `LoadEmbedded`. No process IO |
| Includes | Root `$include` in 0.2, directory-locked. The name is reserved in 0.1 |
| Decrypt | A reader wrapper for a satellite. The facade does not implement age or sops |
| Fingerprint | FNV-1a of the redacted canonical JSON on `LoadReport` |
| Env case and separator | Default separator `__`. Env segments fold to `snake_case`. File keys stay case-sensitive. Ambiguous folds error. Single `_` is rejected; `LossyUnderscore` is not available in 0.1 |
| Unknown keys | Warnings on `HasLoadReport`. `DenyUnknownKeys` and `#[config(deny_unknown)]` opt into errors |
| Interpolation | Off in every default preset. `#[config(interpolate)]` per field. Expanded nodes are marked. No secret and no file scheme in the interpolator |
| Secrets in the tree | References only. Bytes exist in `Secret<T>` after resolve |
| Library env names | `env` is a suffix under the mount path. `env_raw` is the process-global form and must be unique in the root |
| Getters and reload | `#[cgp_getter]` returns `&T` from a context that owns the value for the borrow. The process holds `ConfigHandle`. Requests hold `Arc` |
| Preset evolution | New behavior is a new preset type. `#[version(N)]` is the fork's preset-module version |
| Handler traits | Plain components. Revisit `cgp_fork::handler` only after the loader has shipped and the fork's handler API is the one we would actually call |
| Serde for `HasFields` | A fork PR is useful and is not on the critical path |
| Cloud SDKs | Satellite crates |
| Process environment | Read once into `EnvSnapshot` at bootstrap. Never `set_var`. Never `dotenv()` |
| Profile keys | `$profiles` and `{stem}.{profile}.{ext}` beside the main file. Ordinary top-level keys are not profiles |
| Null overlay | Stores null. Does not delete the lower key |
| Reload rejection | Any `reload = "restart"` change skips the swap. Empty diffs skip notify |
| Metrics | Tracing events. No metric component in 0.1 |
| Validation attributes | Users derive garde or validator themselves |
| Default features | `std`, `derive`, `toml` |
| 0.1 contents | P1–P5. Watch and migrations are 0.2. Remote and cloud are 0.3+ |
| License | `AGPL-3.0-or-later` for all published crates; root `LICENSE` |
| MSRV / toolchain | Rust **1.99** (`rust-version` and `rust-toolchain.toml`) |

### Risks

1. **Fork dependency.** cgp-fork is independently versioned, and ADR 0001 is still Proposed. Upstream `cgp` is on a 0.8 prerelease. *Mitigation:* depend on the prelude surface; keep `cgp_config::cgp` as the only re-export; weekly CI against the fork's `main`; a fork major is our major.
2. **Checked-by-default wiring vs parametric components** (§1.3). *Mitigation:* `#[unchecked]` plus `config_check!` is the documented happy path, and preset rustdoc shows it. A fork PR that skips `UseDelegate` entries in the `()` check would let us drop `#[unchecked]` later.
3. **Compile times and long type errors.** *Mitigation:* short keys, `on_unimplemented` on our traits, the 0.2 median compile-time gate.
4. **CGP learning curve.** *Mitigation:* `quick::load`, presets, and a 0.1 guide whose first chapter never asks the reader to write `delegate_components!`.
5. **YAML ecosystem churn.** *Mitigation:* YAML is not in 0.1. When it lands, the backend is private to the format crate, and alias expansion is off.
6. **notify 9.** Stay on notify 8 behind `ChangeWatcher` until 9 is stable.
7. **Secret leakage through a user `Debug` on a wrapper** that stores a raw `String`. *Mitigation:* `Secret<T>` is the supported container; the redaction test covers our types; a custom lint is a stretch goal after 0.1. The tree still cannot leak bytes it does not store.
8. `**#[cgp_getter]` and `ArcSwap`.** *Mitigation:* the split between `ConfigHandle` and the request context is part of the 0.2 guide and of the example. The API does not offer a `&T` getter on the long-lived context.
9. **Absent `$version` means 1,** so a current-schema file that omits the key will be migrated once 0.2 is on. *Mitigation:* templates write `$version`; the loader warning `AssumedVersion` is on by default; the book says to stamp files.
10. **Detached CGP spawn discards panics.** *Mitigation:* `run_reload_loop` is spawned by the app runtime and `catch_unwind`s each iteration.

---

## Appendix A — prototype index (`/workspace/cgp-config-proto`)

The prototype validated the CGP mechanics. The normative pipeline, error type, and presets in this document replace it where they disagree.

| Item | What the prototype showed | Normative rule now |
|---|---|---|
| `HasConfigValueType` | abstract tree type | same, without a `Debug` bound; nodes are `Arc` |
| `CanLoadSource<Source>` + `UseDelegate` | per-marker dispatch works | same. Providers are `SourceLoader<ThatMarker>`, not `SourceLoader<Source>` for all `Source` |
| `CollectInOrder`, `LoadLayered<Product![..]>` | type-level fold works | same fold. Merge returns `Result`. File layers pass through `CanPrepareLayer` |
| `DeepMergeJson`, serde, per-type validation | merge and `UseDelegate` validation work | `DeepMerge` is fallible on type conflicts. Deserializer is type-directed for env strings. Unknown keys warn |
| `ErrorRaiserComponent` listing `io::Error` and `toml::de::Error` | per-error-type raising works in CGP | built-in providers raise `ConfigError` only, so apps do not list parser errors |
| `JsonConfigPreset` | `cgp_preset!` with `type ConfigValue = Value` | `CoreConfigPreset` / `TwelveFactorPreset` as in §6.15 |
| `HasHttpConfig` + projection | getter consumption works | same, on a context that can return `&T`. Reload uses `ConfigHandle` |
| `CanReloadConfig` + `#[cgp_async(Send)]` | `Send` future that borrows `&self` | loop owns `Arc<Ctx>` and is not launched with `spawn_task` |
| `CanResolveSecret` (`mock`) | generated mocks work | same. Resolvers write `Secret<T>`, not the tree |
| `#[unchecked]` + `check_components!` | workaround for the `()` check | `config_check!` wraps that workaround |

ADR-001 records this table so the prototype's `LoadTomlFile` sketch (generic over every `Source`, `NotFound` always `Ok(None)`, infallible merge, interpolate-then-migrate) is not copied into the crates.
