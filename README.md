# cgp-config

Context-generic, layered, typed configuration for Rust, built on
[Context-Generic Programming](https://contextgeneric.dev/) (`cgp-fork` 1.1).

Your *context* (a plain struct) picks, at compile time, where configuration comes from,
how layers merge, how values deserialize, and how the result is validated. Every stage is a
CGP component with swappable providers, so a test context, a staging binary, and a
production binary can wire different sources and checks with no `cfg!` sprawl and no `dyn`.

> **Status: 0.1.0-alpha.1 (phases P0–P5 of the [design plan](docs/design-plan.md)).**
> The component set, value tree, merge, deserializer, errors, `CoreConfigPreset` (P1);
> TOML/JSON formats, file/env/`.env`/CLI sources, `Bootstrap`, and the twelve-factor
> presets (P2); `#[derive(ConfigSection)]`, unit parsers, and the validation combinators
> and adapters (P3); `Secret<T>` / `LazySecret<T>`, the `env:` / `file:` / `docker:`
> resolvers, redaction, fingerprints, `explain`, and the `miette` diagnostic (P4);
> `#[config(getter)]` / `#[config(partial)]`, `ProjectSection`, `quick::load`, the
> multi-crate example, and the 0.1 book (P5) are in place. Reload, migrations, and
> satellite crates (P6–P8) are not implemented yet.

## What is here

| Crate | Contents |
|---|---|
| [`cgp-config`](crates/cgp-config) | Facade: re-exports and `prelude`. Applications depend on this |
| [`cgp-config-core`](crates/cgp-config-core) | `#![no_std]` component traits, `KeyPath`, `Origin`/`Meta` provenance, `ConfigError`, `LoadReport`/`LoadWarning`, `LoadLimits`, the built-in source markers, pipeline providers (`LoadLayered`, `CollectInOrder`, ...), `config_check!` |
| [`cgp-config-value`](crates/cgp-config-value) | `ConfigValue` (`Arc` nodes with `Meta`), merge policies (`DeepMerge`, `ShallowReplace`, `AppendArrays`, `JoinBaseWins`, `AdjoinArrays`, `MergeArraysByKey`), the provenance-aware serde deserializer, `PrepareFileLayer`, `CoreConfigPreset` with its `DefaultPreparers` table |
| [`cgp-config-formats`](crates/cgp-config-formats) | `#![no_std]` TOML and JSON parsers with spans and duplicate-key rejection, redacting renderers, `EnabledFormats` |
| [`cgp-config-providers`](crates/cgp-config-providers) | `Bootstrap`, file search, file/env/`.env`/CLI sources, `TwelveFactorPreset` and its variants, `SecretsPreset` (feature `secrets`), `StdLoader`/`MemoryLoader`, garde/validator adapters, `ConfigDiagnostic` (feature `miette`) |
| [`cgp-config-derive`](crates/cgp-config-derive) | `#[derive(ConfigSection)]` (implementation in `cgp-config-derive-impl`) |
| [`cgp-config-secrets`](crates/cgp-config-secrets) | `Secret<T>`, `LazySecret<T>`, `env:` / `file:` / `docker:` resolvers (feature `secrets` on the facade) |
| `fuzz` | cargo-fuzz targets (not in the workspace) |
| `examples/multi-crate-workspace` | `no_std` HTTP and database sections, and an app that mounts both (not published) |
| `book` | The 0.1 guide (`cargo xtask book` when `mdbook` is installed) |
| `xtask` | `cargo xtask ci` and friends (not published) |

The load pipeline is **collect** (for each source: load → prepare → merge onto the
accumulator) → **interpolate** (off by default) → **deserialize** → **resolve secrets** →
**validate**. Each arrow is a component.

## Quick look

With the derive and a preset (see the facade docs for the full example):

```rust,ignore
#[derive(serde::Deserialize, ConfigSection)]
struct AppConfig {
    #[config(default = 8080, env_raw = "PORT")]
    port: u16,
    #[config(default = "30s", parse = "duration")]
    timeout: std::time::Duration,
}

let config: AppConfig = cgp_config::from_toml_str("port = 9090")?;
```

By hand, with only the core crates:

```rust
use cgp_config::prelude::*;
use cgp_config::cgp::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_config::cgp::extra::error::RaiseFrom;

#[derive(serde::Deserialize)]
struct AppConfig { port: u16 }

// `Defaults` is a built-in source marker (from the prelude); `BuiltIn` serves it.
struct BuiltIn;

#[cgp_impl(BuiltIn)]
impl SourceLoader<Defaults>
where
    Self: HasConfigValueType<ConfigValue = Value> + HasErrorType,
{
    fn load_source(&self, _: PhantomData<Defaults>) -> Result<Option<Value>, Self::Error> {
        Ok(Some(Value::map([("port", Value::from("8080"))]).with_origin(Origin::Defaults)))
    }
}

#[derive(HasField)]
struct Loader { load_report: LoadReport }

delegate_components! {
    #[unchecked]
    Loader {
        preset CoreConfigPreset,
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        SourceLoaderComponent: BuiltIn,
        ConfigLoaderComponent: LoadLayered<Product![Defaults]>,
    }
}

// Compile-time check of the wiring with the real config type and sources.
config_check! { Loader { config: AppConfig, sources: [Defaults] } }

let config: AppConfig = Loader { load_report: LoadReport::new() }.load_config()?;
```

Errors carry the path, the winning origin, and what it overrode:

```text
http.port: expected u16, found string "http" at env APP__HTTP__PORT (overrides config/app.toml)
cannot merge http: map from config/app.toml vs string from env APP__HTTP
unknown key http.prot at config/app.toml; did you mean "port"?
unknown keys (2): http.prot at config/app.toml (did you mean "port"?); nmae at config/app.toml (did you mean "name"?)
```

Every built-in source marker (`Defaults`, `ConfigFile`, `EnvVars`, ...) has a preparer in
the preset's `DefaultPreparers` table; a context that adds its own marker wires its own
`UseDelegate` table, and forgetting one is a compile error. Env strings are coerced by
the target type, so give fields concrete types: untagged enums and `#[serde(flatten)]`
see `"8080"` as a string.

Applications depend on `cgp-fork` directly as well, because the CGP macros expand to
`::cgp_fork` paths ([ADR-001](docs/adr/0001-prototype-findings.md), finding 6).

## `no_std`

`cgp-config-core` builds without `alloc` (component traits, `LoadLimits`, `StaticKeyPath`),
and `cgp-config-value` and the facade build with `alloc` only. CI builds them for
`thumbv7em-none-eabihf`.

## Development

```sh
cargo xtask ci        # fmt, clippy, tests, feature matrix, no_std, docs, cargo-deny
cargo xtask miri      # Miri on core and value (nightly, isolation on)
cargo xtask coverage  # line-coverage gate: 95% on cgp-config-value (core printed)
cargo xtask semver    # cargo-semver-checks against $SEMVER_BASELINE (default origin/main)
cargo xtask fork-main # build and test against cgp-fork's main branch
```

Toolchain 1.99.1 is pinned in `rust-toolchain.toml` (MSRV 1.99); `fmt` and `miri` use
nightly. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Documents

* [Design plan](docs/design-plan.md): the full design and the phased roadmap.
* [ADR-001](docs/adr/0001-prototype-findings.md): prototype findings and P1 decisions.
* [CHANGELOG](CHANGELOG.md)

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
