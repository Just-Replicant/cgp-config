# cgp-config-core

Component traits, key paths, provenance, the `ConfigError` type, and load limits for
[`cgp-config`](../cgp-config). `#![no_std]`; the `alloc` feature adds the owned types.

Applications depend on the `cgp-config` facade. Libraries that only name the traits may
depend on this crate directly. This crate is part of the public, semver-stable API.

License: AGPL-3.0-or-later.
