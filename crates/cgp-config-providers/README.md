# cgp-config-providers

The `std` sources and presets of [`cgp-config`](../cgp-config) (design plan phase P2).
Applications depend on the `cgp-config` facade, which re-exports this crate.

## Sources

| Marker | Provider | Reads |
|---|---|---|
| `Defaults` | `LoadDefaults` | `#[config(default = ...)]` values from the root `ConfigMeta` |
| `ConfigFile` | `LoadFileAuto` | the explicit `--config`/`{PREFIX}_CONFIG` file, or the first `{app}.{toml,json}` found in `$CONFIGURATION_DIRECTORY`, the working directory, the executable's directory, then the platform config directory |
| `ProfileFile` | `LoadProfileFile` | `{app}.{profile}.{ext}` next to the config file (optional) |
| `LocalFile` | `LoadLocalFile` | `{app}.local.{ext}` next to the config file (optional, never commit it) |
| `ConfigDir` | `LoadConfDir` | `conf.d/*.{toml,json}` in lexical order |
| `EnvVars` | `LoadEnvVars` | `{PREFIX}__A__B=...` from the captured `EnvSnapshot`, plus `env_raw` names |
| `EnvDocument` | `LoadEnvDocument` (`json`) | one JSON document in `{PREFIX}_DOCUMENT` |
| `DotEnv` | `LoadDotenv` (`dotenv`) | `{PREFIX}__...` entries of a `.env` file, never the process environment |
| `CliArgs` | `LoadCliOverrides` | `--set key.path=value` overrides |
| `InMemory` | `LoadFromField<Symbol!("memory_layer")>` | a document held by the context (tests) |
| `Embedded` | `LoadEmbedded<Fmt>` (wired by the context) | a document compiled into the binary |
| `DynSources` | `LoadDynSources` | sources chosen at run time |

Every layer is canonicalized against the root `ConfigMeta` (serde keys, flattened
sections under their field), so layers merge key by key and errors keep the
`file:line:column` of the value that won. Files are read with a torn-read retry, size
limits apply before parsing, and on Unix a world-writable file or directory is reported as
a warning.

## Bootstrap

`Bootstrap` resolves what must be known before loading: the env prefix (`my-app` →
`MY_APP`), a snapshot of the environment taken once, the active profile, the config file
location, an optional `.env` path, and CLI overrides. `--config`, `--profile`,
`--env-prefix`, and `--set` are read from plain arguments, or from `clap` with the `clap`
feature (`cli::args()`, `cli::augment`). Tests use `with_env` and never touch the process
environment.

## Presets

* `TwelveFactorPreset`: defaults → config file → profile file → env.
* `TwelveFactorLocal`, `TwelveFactorConfDir`: add the local file or `conf.d`.
* `TwelveFactorDotenv`, `TwelveFactorCli`, `TwelveFactorDotenvCli` (features `dotenv`,
  `clap`): add `.env` before env and CLI overrides last.
* `InterpolatingPreset` (`interpolate`): `${...}` expansion of opted-in fields.
* `TestConfigPreset`: in-memory document plus env, no file IO.

`StdLoader` and `MemoryLoader` are ready-made contexts; `from_toml_str`,
`from_json_str`, and `from_defaults` cover the common one-liners.

## Validation adapters

`ValidateWithGarde` (feature `validate-garde`) and `ValidateWithValidator` (feature
`validate-validator`) map their crates' reports onto `ValidationErrors` with config key
paths and origins. Combine them with `AllOf` and `Nested` from `cgp-config-core`.

## Example

```rust
use cgp_config_core::EnvSnapshot;
use cgp_config_providers::env::env_layer;

let env: EnvSnapshot = [("APP__HTTP__PORT", "8080"), ("OTHER", "x")].into_iter().collect();
let layer = env_layer(&env, "APP", None)?.expect("one variable matched");
let port = layer.get_path(&"http.port".into()).expect("nested by `__`");
assert_eq!(port.as_str(), Some("8080"));
# Ok::<(), cgp_config_core::ConfigError>(())
```

License: AGPL-3.0-or-later.
