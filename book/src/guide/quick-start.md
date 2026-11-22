# Quick start

An application with the default features (`std`, `derive`, `toml`) can load a section
without writing a context:

```rust
use cgp_config::prelude::*;

#[derive(Debug, serde::Deserialize, ConfigSection)]
struct AppConfig {
    #[config(default = 8080)]
    port: u16,
    #[config(default = "demo")]
    name: String,
}

let (config, report): (AppConfig, _) = cgp_config::quick::load("myapp")?;
```

`load` reads the process environment once and searches the filesystem for `myapp.toml`
(see [Twelve-factor](twelve-factor.md)). The prefix is the app name uppercased, so a
nested key is `MYAPP__HTTP__PORT`. The profile defaults to `default`.

`load` is for the application binary. Libraries do not call it. Tests call
`quick::load_from` with an `EnvSnapshot` and an in-memory file map. That path does not
touch the process environment or the disk. A file that sets `"$version" = 1` records
`LoadWarning::DocumentVersion`; a file that omits `$version` records `AssumedVersion`.

`StdLoader::builder()` is the same twelve-factor load with the knobs named:

```rust
let loader = StdLoader::builder()
    .app_name("myapp")
    .file("cfg.toml")
    .env_prefix("APP")
    .profile("prod")
    .env(EnvSnapshot::new())
    .build::<AppConfig>()?;
let config = loader.load::<AppConfig>()?;
```

The prefix is stored without a trailing separator (`APP`, not `APP_` or `APP__`). An
empty prefix, or one that ends with `_`, is rejected. The builder does not read the
process environment unless you call `capture_env()`.
