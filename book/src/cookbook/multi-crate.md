# Multi-crate workspace

`examples/multi-crate-workspace` is the 0.1 layout.

* `example-http-server` and `example-db-pool` are `#![no_std]` libraries. Each derives
  `ConfigSection` with `#[config(section = "...", getter)]`. They depend on `cgp-config`
  with `default-features = false, features = ["derive"]`.
* `example-multi-crate-app` depends on both, and on the facade with `std`, `derive`,
  and `toml`. `AppConfig` mounts the two sections with `#[config(nested, mount = "...")]`.
  `Request` holds `Arc<AppConfig>` and delegates `HttpConfigGetterComponent` and
  `DbConfigGetterComponent` to `WithProvider<ProjectSection<Path!(@config.http)>>` and
  `Path!(@config.db)`.

The app test loads with `quick::load_from`: env `MYAPP__HTTP__PORT` and
`MYAPP__DB__MAX_CONNECTIONS`, plus a toml file for `name` and `[db] host`. A second test
mounts `HttpConfig` twice, as `api_http` and `admin_http`.

Duplicate mounts in one struct, and duplicate `env_raw` names, are compile errors. The
facade's trybuild suite locks both (`fail_mounts`, `fail_keys`).
