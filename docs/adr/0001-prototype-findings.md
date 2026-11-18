# ADR-001: Prototype findings, and where the design plan supersedes the prototype

* Status: Accepted
* Date: 2026-11-17
* Phase: P0 (spike); the findings marked *new* came from building P1
* Design plan: [`docs/design-plan.md`](../design-plan.md) (§1.3, §1.4, Appendix A)

## Context

Before the crates were written, a throwaway prototype (`/workspace/cgp-config-proto`, not
part of this repository) compiled a first cut of the CGP components on toolchain 1.99.1 and
on cgp-fork's MSRV 1.89. It checked that the mechanics this library depends on work in
`cgp-fork` 1.1. The design plan then changed several of the prototype's choices. This
record keeps what the prototype proved, and states which of its choices must not be
carried into the crates.

## Findings from the prototype (§1.3)

1. **Checked-by-default wiring breaks components with generic parameters.** A
   non-generic `delegate_components!` emits a `__CanUse{Context}` check whose parameters
   are `()`. For `CanLoadSource<Source>`, `CanValidateConfig<Config>`, and
   `CanRaiseError<E>` wired through `UseDelegate<new Table { .. }>`, that check asks for a
   `()` entry and fails.
   **Rule:** contexts that wire parametric config components mark the table
   `#[unchecked]` and check it with real parameters. The supported form is
   `config_check!` (in `cgp-config-core`), which expands to `check_components!` for the
   loader, interpolator, deserializer, secrets, and validator components at the config
   type, and the source and preparer components at every listed source marker.
2. **A type-level source list folds.** A `Cons<Source, Rest>` / `Nil` provider over
   `Product![..]` works with `#[cgp_impl]`. Implemented as `CollectInOrder` and
   `LoadLayered<Sources>`.
3. **`#[cgp_async(Send)]` gives `Send` futures** for a generic `CanReloadConfig<Config>`
   when the context is `Sync`. The future borrows `&self` and is not `'static`; a spawned
   reload loop must own `Arc<Context>`. `CanReloadConfig` and `CanWatchChanges` are
   declared in P1 so 0.2 can add providers without a breaking change.
4. **The `mock` feature** generates `MockX` / `XMock` for components. Usable for secret
   stores and remote sources. The facade forwards the `mock` feature.
5. **MSRV.** The prototype compiled on 1.89. The plan's normative MSRV (§16) is **1.99**
   (`rust-version = "1.99"`, toolchain pinned to 1.99.1); §0's table still says 1.89, and
   §16 wins. Raising the floor above cgp-fork's 1.89 is deliberate.

## Where the plan supersedes the prototype (Appendix A)

| Prototype item | What it showed | Normative rule (implemented in P1 unless noted) |
|---|---|---|
| `HasConfigValueType` | abstract tree type | Same, without a `Debug` bound. The default tree is `ConfigValue` with `Arc` nodes |
| `CanLoadSource<Source>` + `UseDelegate` | per-marker dispatch works | Same. Providers implement `SourceLoader<ThatMarker>`, never `SourceLoader<Source>` for every `Source` |
| `CollectInOrder`, `LoadLayered<Product![..]>` | type-level fold works | Same fold. Merge returns `Result`. Every layer passes `CanPrepareLayer` (identity until the P2 file preparers) |
| `DeepMergeJson`, serde, per-type validation | merge and `UseDelegate` validation work | `DeepMerge` fails with `ConfigError::MergeConflict` on shape changes. The deserializer is type-directed (env strings parse into bools, numbers, enums, sequences). Unknown keys warn on the `LoadReport` |
| `ErrorRaiserComponent` listing `io::Error`, `toml::de::Error` | per-error-type raising works | Built-in providers raise only `ConfigError`, so applications never list parser errors |
| `JsonConfigPreset` | `cgp_preset!` with `type ConfigValue = Value` | `CoreConfigPreset` (P1); `TwelveFactorPreset` arrives with the sources (P2) |
| `HasHttpConfig` + projection | getter consumption works | Same, on a context that can return `&T`. Reload uses `ConfigHandle` (P6) |
| `CanReloadConfig` + `#[cgp_async(Send)]` | `Send` future borrowing `&self` | The loop owns `Arc<Ctx>` and is not launched with `spawn_task` (P6) |
| `CanResolveSecret` (`mock`) | generated mocks work | Same. Resolvers write `Secret<T>`, never the tree (P4). P1 wires `DenySecrets` |
| `#[unchecked]` + `check_components!` | workaround for the `()` check | `config_check!` wraps the workaround |

The prototype's `LoadTomlFile` sketch must **not** be copied into the crates: it was generic
over every `Source`, returned `Ok(None)` for every `NotFound`, merged infallibly, and
interpolated before migrating. The plan's pipeline is: collect (load → prepare → merge per
layer) → interpolate (opt-in only) → deserialize → resolve secrets → validate.

## New findings while building P1

6. **Wiring macros name `::cgp_fork`, so applications depend on `cgp-fork` directly.**
   `#[cgp_impl]`, `delegate_components!`, `cgp_preset!`, and `check_components!` expand to
   paths through the fork crate as the *calling* crate sees it. The `cgp_config::cgp`
   re-export is enough for naming types in signatures but not for macro expansion.
   Applications list `cgp-fork = { version = "1.1", default-features = false, features = ["alloc"] }`
   next to `cgp-config`. Documented in the facade crate docs and README.
7. **`#[cgp_auto_getter]` returning `&str` names `String` unqualified.** In a `no_std`
   crate the module must `use alloc::string::String;` (done in
   `cgp-config-core/src/components/profile.rs`).
8. **Presets replay their component keys at the use site.** `preset CoreConfigPreset`
   inside `delegate_components!` expands the preset's `with_components!` macro, which names
   each component key unqualified in the caller's scope. Callers therefore need the
   component names imported; `use cgp_config::prelude::*;` does that. Provider types are
   resolved in the defining crate and do not need importing.
9. **`#[diagnostic::on_unimplemented]` on a component trait is copied onto the generated
   provider impls,** which rustc warns about. The fork already generates
   `on_unimplemented` messages for components, so our traits carry none of their own.
   The plan's §11 note ("our traits carry `on_unimplemented` messages") is satisfied by the
   fork's generated messages.
10. **`cgp_preset!` accepts no doc attributes, and `#[allow]` on the invocation is
    ignored.** `CoreConfigPreset` is documented in the `preset` module docs, and that
    module carries `#![allow(missing_docs)]` for the generated items.
11. **`cgp_type` does not generate a `{Name}Of<Context>` alias.** `ConfigValueOf<C>` is
    written by hand in `cgp-config-core`.
12. **`#[cgp_auto_error]` cannot express optional parts or suppress messages.**
    `ConfigError`'s `Display` is hand-written so that `Type` errors on secret nodes print a
    fixed message and the location and override chain are shown only when known.
13. **A custom component name needs the name in `#[cgp_impl]`:**
    `#[cgp_impl(LoadLimitsField: LoadLimitsComponent)]`.

## Decisions taken in P1 that differ from the letter of the plan

* **`Meta` is stored inline in each `Arc` node,** not in a side arena indexed by node id
  (§11). Structural sharing across layers is kept (unchanged subtrees are the same `Arc`;
  tests assert `ptr_eq`), and the inline layout avoids an id-remapping step on every merge.
  An arena remains an internal optimization that does not change the public API
  (`ConfigValue::meta()` returns `&Meta` either way).
* **`ConfigError` boxes its `Meta` payloads** (`Option<Box<Meta>>` / `Box<Meta>`) to keep
  `Result<_, ConfigError>` small (clippy `result_large_err`). Constructors take `Meta` by
  value; `ConfigError::meta()` returns `Option<&Meta>`.
* **Load limits are a component** (`LoadLimitsComponent`, provider `DefaultLoadLimits` or
  `LoadLimitsField`) rather than an auto-getter, so the preset can provide a default and a
  context can still take them from a field.
* **The pipeline's secrets stage is `CanResolveSecrets<Config>`
  (`SecretsResolverComponent`)**; the per-scheme resolver of §6 arrives with P4.
  `CoreConfigPreset` wires `DenySecrets`, which is a no-op until secret references exist.
* **Unknown-key policies (`WarnUnknownKeys`, `DenyUnknownKeys`, `IgnoreUnknownKeys`) landed
  in P1** although the roadmap lists them in P3: the preset's deserializer needs a policy.
* **A string may replace an array in a merge** (env layers carry arrays as JSON text that
  the deserializer parses later). Every other shape change is a `MergeConflict`.
* **Booleans parse from `true`/`false` in any ASCII case only.** No `yes`/`no`/`1`/`0`, no
  trimming. Numbers parse with `FromStr` (no trimming).
* **The facade's default features are `["std"]`** until the `derive` and `toml` crates
  exist (P2/P3); the plan's final default set is `std`, `derive`, `toml`.
* **Deferred, by phase:** `FinalizeOptionalAll` and the `heapless` tier (P3, with the
  derive), duration and byte-size parsers (P3), `LoadReport::fingerprint` (P4), and
  `from_toml_str` / `from_json_str` (P2/P3). The tracing/no-op logger wiring of §12 lands
  with the first providers that log (P2).

## Consequences

* Every example and test context uses `#[unchecked]` plus `config_check!`.
* Applications depend on `cgp-config` **and** `cgp-fork`, and import `cgp_config::prelude::*`.
* The prototype's API shapes listed above are not reintroduced; reviewers can cite this
  ADR when they appear in a pull request.
