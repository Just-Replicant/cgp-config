# Semver surface

The crate is `0.1.0-alpha.1`. Pre-1.0, the public surface is still the contract
`cargo xtask semver` checks:

* consumer traits, provider names, and preset names
* feature names
* `ConfigError` variants and `code()` strings
* `ConfigMeta` and the `#[config]` attribute syntax
* the reserved keys `$version`, `$profiles`, and `$include`
* the published crates `cgp-config`, `cgp-config-core`, `cgp-config-value`,
  `cgp-config-formats`, `cgp-config-providers`, `cgp-config-secrets`, and
  `cgp-config-derive`

`cgp-config-derive-impl` and the value-tree arena have no stability promise.
`MapLoader`, the context behind `quick::load_from`, is `#[doc(hidden)]`.

`cargo xtask semver` runs `cargo-semver-checks` against `SEMVER_BASELINE`, which
defaults to `origin/main`. That revision is the baseline. This alpha does not invent a
git tag for it. The check is advisory until 0.1.0. CI does not fail the build on it.

Adding a component or a preset is non-breaking. Changing `TwelveFactorPreset`'s layer
list is breaking and would be a new preset type. A major bump of `cgp-fork` is a major
bump here, because CGP types appear in public traits. The fork requirement is
`cgp-fork = "1.1"`.

MSRV is 1.99. An MSRV bump is a minor release.
