# cgp-config

Context-generic, layered, typed configuration for Rust, built on
[CGP](https://contextgeneric.dev/) (`cgp-fork`). See the [workspace README](../../README.md)
for an overview and the crate documentation for a complete example.

```toml
[dependencies]
cgp-config = "0.1.0-alpha.1"
cgp-fork   = { version = "1.1", default-features = false, features = ["alloc"] }
serde      = { version = "1", features = ["derive"] }
```

License: AGPL-3.0-or-later.
