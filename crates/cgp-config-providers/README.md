# cgp-config-providers

The `std` sources and presets of [`cgp-config`](../cgp-config) (design plan phase P2).
Applications depend on the `cgp-config` facade, which re-exports this crate.

## Sources

| Marker | Provider | Reads |
|---|---|---|
| `Defaults` | `LoadDefaults` | `#[config(default = ...)]` values from the root `ConfigMeta` |
| `ConfigFile` | `LoadFileAuto` | the explicit `--config`/`{PREFIX}_CONFIG` file, or the first `{app}.{toml,json}` found in `$CONFIGURATION_DIRECTORY`, the working directory, the executable's directory, then the platform config directory |
| `ProfileFile` | `LoadProfileFile` | `{stem}.{profile}.{ext}` next to the config file (optional; `RequireActiveProfile` makes a missing profile an error) |
| `LocalFile` | `LoadLocalFile` | `{stem}.local.{ext}` next to the config file (optional, never commit it) |
| `ConfigDir` | `LoadConfDir` | `conf.d/*.{toml,json}` in lexical order (symlinks only when they resolve inside `conf.d`) |
| `EnvVars` | `LoadEnvVars` | `{PREFIX}__A__B=...` from the captured `EnvSnapshot` (segments fold to `snake_case`), plus `env_raw` names |
| `EnvDocument` | `LoadEnvDocument` (`json`) | one JSON document in `{PREFIX}_DOCUMENT` |
| `DotEnv` | `LoadDotenv` (`dotenv`) | `{PREFIX}__...` entries of a `.env` file, never the process environment |
| `CliArgs` | `LoadCliOverrides` | `--set key.path=value` overrides; `servers[1].host=x` patches one array element |
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
feature (`cli::args()`, `cli::augment`, `cli::config_command`). Tests use `with_env` and
never touch the process environment.

Name the layer list the loader will use (`Bootstrap::layers::<L>()`, types in `layers`),
so bootstrap and loader agree: `--set` is rejected unless the list has `CliArgs`, and
`StdLoader::new` refuses a `{PREFIX}_DOCUMENT` or an explicit `.env` path its list would
ignore. A search that finds no file leaves the layer absent unless `require_file(true)`.

## Presets

* `TwelveFactorPreset`: defaults → config file → profile file → env.
* `TwelveFactorLocal`, `TwelveFactorConfDir`: add the local file or `conf.d`.
* `TwelveFactorDotenv`, `TwelveFactorCli`, `TwelveFactorDotenvCli` (features `dotenv`,
  `clap`): add `.env` before env and CLI overrides last.
* `InterpolatingPreset` (`interpolate`): `${...}` expansion of opted-in fields.
* `StrictPreset`: `TwelveFactorPreset` with `DenyUnknownKeys` and
  `RequireActiveProfile`.
* `TestConfigPreset`: extends `CoreConfigPreset` with defaults, an in-memory document,
  and env (`TestLayers`), no file IO and no logging. Mock secret schemes arrive with P4.

`StdLoader<Layers>` (default `TwelveFactorLayers`) and `MemoryLoader` are ready-made
contexts:

```rust,ignore
let boot = Bootstrap::new("app")
    .layers::<TwelveFactorCliLayers>()
    .capture_env()?
    .from_args(std::env::args().skip(1))?
    .finish();
let loader = StdLoader::<TwelveFactorCliLayers>::with_layers(boot)?.with_section::<AppConfig>();
let config: AppConfig = loader.load()?;
```

`from_toml_str`, `from_json_str`, `from_*_with` (secret resolver, warnings), and
`from_defaults` cover the common one-liners.

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
