# `no_std`

`cgp-config-core`, `cgp-config-formats`, and the facade are `#![no_std]`. A library that
only derives a section links no std IO. Depend on the facade with
`default-features = false` and `features = ["derive"]`, and on `cgp-fork` with
`default-features = false`. Serde needs `default-features = false` plus `derive` and
`alloc`.

`derive` implies `alloc`. It does not imply `std` and it does not pull
`cgp-config-providers`. `quick::load`, `StdLoader`, `Bootstrap`, and the filesystem
reader are `std`.

Without `alloc`, the facade keeps the component traits, `LoadLimits`, and
`StaticKeyPath`. That is enough to name a context, not enough to parse a document.

`examples/multi-crate-workspace/http-server` is the check: `#![no_std]`, the derive, and
a unit test of `HttpConfig::META`.
