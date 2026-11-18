# cgp-config-value

The default configuration tree for [`cgp-config`](../cgp-config): `ConfigValue` (`Arc`
nodes, each with `Meta` provenance), fallible merge policies, and a type-directed serde
`Deserializer` that keeps paths and origins in its errors, collects unknown keys, and never
echoes secret values. Also defines `CoreConfigPreset`.

`#![no_std]` with `alloc`. Applications depend on the `cgp-config` facade, which
re-exports this crate.

License: AGPL-3.0-or-later.
