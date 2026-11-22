# CLI tools

Enable `clap`. `Bootstrap` then accepts `--config`, `--profile`, `--env-prefix`, and
`--set key=value`. `TwelveFactorCli` is the preset that loads those overrides after the
env layer.

```rust
let loader = StdLoader::builder()
    .app_name("myapp")
    .capture_env()
    .build::<AppConfig>()?;
```

`--set` is rejected unless the layer list contains `CliArgs`. A path such as
`servers[1].host` patches one array element. The builder's `env_prefix` is `APP`, with
no trailing underscore.

A CLI that must not touch the process environment (tests, a library example) passes
`env(snapshot)` and does not call `capture_env()`.
