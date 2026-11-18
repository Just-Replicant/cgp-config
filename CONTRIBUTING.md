# Contributing

Thanks for helping. The design lives in [docs/design-plan.md](docs/design-plan.md);
decisions that change it are recorded as ADRs in [docs/adr](docs/adr).

## Setup

* Rust 1.99.1 (pinned by `rust-toolchain.toml`; rustup installs it on first use) with the
  `thumbv7em-none-eabihf` target.
* A nightly toolchain with `rustfmt` and `miri` for formatting and Miri.
* `cargo install cargo-hack cargo-deny cargo-llvm-cov --locked`.

## Before opening a pull request

```sh
cargo xtask ci
```

runs, in order: `fmt` (nightly rustfmt, check only), `clippy` (`-D warnings`, all features
and no default features), `test` (all features plus each feature tier), `hack`
(`--each-feature` on the facade, `--feature-powerset` on core and value), `nostd`
(builds for `thumbv7em-none-eabihf`), `docs` (broken intra-doc links denied), and `deny`.
Miri (`cargo xtask miri`) and the coverage gate (`cargo xtask coverage`, 95% lines on
`cgp-config-value`) run in their own CI jobs.

## Rules of the code base

* `cgp-config-core` and `cgp-config-value` are `#![no_std]`: import from `core` and
  `alloc`, never `std` (clippy enforces this). No `std::env`, no `std::fs` there.
* No `unsafe` (`unsafe_code = "forbid"`).
* Every public item is documented; every error variant has a stable `code()`.
* Never echo a value from a node whose `Meta::secret` is set: not in `Display`, not in
  `Debug`, not in warnings.
* Contexts with parametric components use `#[unchecked]` plus `config_check!`
  (ADR-001, finding 1).
* Add a CHANGELOG entry under *Unreleased* for user-visible changes.

## License

By contributing you agree that your contributions are licensed under AGPL-3.0-or-later.
