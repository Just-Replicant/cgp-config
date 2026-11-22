# Twelve-factor

`TwelveFactorPreset` layers, lowest precedence first:

1. `Defaults` — `#[config(default = ...)]` from the root `ConfigMeta`, including nested
   sections.
2. `ConfigFile` — the file bootstrap selected.
3. `ProfileFile` — `{stem}.{profile}.{ext}` beside that file (`myapp.prod.toml`).
4. `EnvVars` — `{PREFIX}__PATH` from the one `EnvSnapshot`, plus `env_raw` names.

`StdLoader` and `quick::load` use that list. `TwelveFactorLocal` adds
`{stem}.local.{ext}`. `TwelveFactorConfDir` adds `conf.d`. `TwelveFactorDotenv` and
`TwelveFactorCli` add `.env` and `--set` (features `dotenv` and `clap`). Those are
separate presets. The production preset does not read a local file.

Search order for the config file (`SearchPath::standard`):

1. `{PREFIX}_CONFIG` when that variable is in the snapshot.
2. `$CONFIGURATION_DIRECTORY`.
3. The working directory: `{app}.toml`, `config.toml`, `config/{app}.toml`.
4. The executable's directory.
5. The platform config directory.

JSON is the same search with `.json` when the `json` feature is on. Bootstrap keys
(`{PREFIX}_CONFIG`, `{PREFIX}_PROFILE`, `{PREFIX}_DOCUMENT`) are removed from the
snapshot before the env layer runs, so they do not become config fields.

A missing file is not an error unless the file was named explicitly or `require_file`
is set. The load can still succeed from defaults and env.

`"$version" = 1` in a file is recorded as `config::warn::document_version` and then
removed. A file with no `$version` is treated as version 1 and records
`config::warn::assumed_version`. Migrations are 0.2.

Unknown keys warn (`config::warn::unknown_key`). `#[config(deny_unknown)]` on a section,
or `DenyUnknownKeys` on the loader, turns them into `config::unknown`.

The process environment is read once, into an `EnvSnapshot`. Nothing in this crate
calls `set_var` or a dotenv crate that mutates the process.
