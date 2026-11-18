# cgp-config

Context-generic, layered, typed configuration for Rust, built on
[Context-Generic Programming](https://contextgeneric.dev/) (`cgp-fork` 1.1).

Your *context* (a plain struct) picks, at compile time, where configuration comes from,
how layers merge, how values deserialize, and how the result is validated. Every stage is a
CGP component with swappable providers, so a test context, a staging binary, and a
production binary can wire different sources and checks with no `cfg!` sprawl and no `dyn`.

> **Status: 0.1.0-alpha.1 (phases P0 and P1 of the [design plan](docs/design-plan.md)).**
> The component set, value tree, merge, deserializer, errors, and `CoreConfigPreset` are
> in place. File, environment, and CLI sources (P2), the derive and validation adapters
> (P3), and secrets (P4) are not implemented yet; until then a context supplies its own
> `SourceLoader` providers.

## What is here

| Crate | Contents |
|---|---|
| [`cgp-config`](crates/cgp-config) | Facade: re-exports and `prelude`. Applications depend on this |
| [`cgp-config-core`](crates/cgp-config-core) | `#![no_std]` component traits, `KeyPath`, `Origin`/`Meta` provenance, `ConfigError`, `LoadReport`/`LoadWarning`, `LoadLimits`, pipeline providers (`LoadLayered`, `CollectInOrder`, ...), `config_check!` |
| [`cgp-config-value`](crates/cgp-config-value) | `ConfigValue` (`Arc` nodes with `Meta`), merge policies (`DeepMerge`, `ShallowReplace`, `AppendArrays`, `JoinBaseWins`, `AdjoinArrays`, `MergeArraysByKey`), the provenance-aware serde deserializer, `CoreConfigPreset` |
| `xtask` | `cargo xtask ci` and friends (not published) |

The load pipeline is **collect** (for each source: load → prepare → merge onto the
accumulator) → **interpolate** (off by default) → **deserialize** → **resolve secrets** →
**validate**. Each arrow is a component.

## Quick look

```rust
use cgp_config::prelude::*;
use cgp_config::cgp::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_config::cgp::extra::error::RaiseFrom;

#[derive(serde::Deserialize)]
struct AppConfig { port: u16 }

struct Defaults;      // a source marker
struct BuiltIn;       // a provider for it

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
```

Applications depend on `cgp-fork` directly as well, because the CGP macros expand to
`::cgp_fork` paths ([ADR-001](docs/adr/0001-prototype-findings.md), finding 6).

## `no_std`

`cgp-config-core` builds without `alloc` (component traits, `LoadLimits`, `StaticKeyPath`),
and `cgp-config-value` and the facade build with `alloc` only. CI builds them for
`thumbv7em-none-eabihf`.

## Development

```sh
cargo xtask ci        # fmt, clippy, tests, feature matrix, no_std, docs, cargo-deny
cargo xtask miri      # Miri on core and value (nightly)
cargo xtask coverage  # line-coverage gate: 95% on cgp-config-value
```

Toolchain 1.99.1 is pinned in `rust-toolchain.toml` (MSRV 1.99); `fmt` and `miri` use
nightly. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Documents

* [Design plan](docs/design-plan.md): the full design and the phased roadmap.
* [ADR-001](docs/adr/0001-prototype-findings.md): prototype findings and P1 decisions.
* [CHANGELOG](CHANGELOG.md)

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
