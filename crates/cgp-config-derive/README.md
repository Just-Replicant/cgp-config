# cgp-config-derive

`#[derive(ConfigSection)]` for [cgp-config](../../README.md). Depend on the facade
(`cgp-config`, feature `derive`, on by default) rather than on this crate; the expansion
refers to `::cgp_config::__private`. Use `#[config(crate = "path")]` when the facade is
renamed or re-exported.

License: AGPL-3.0-or-later.
