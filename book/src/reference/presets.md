# Presets

A preset is a `cgp_preset!` table. Later entries in `delegate_components!` override it.
`use cgp_config::prelude::*` (or `cgp_config_providers::presets::__entries::*` inside that
crate) must be in scope at the use site: the macro replays the provider names there.

| Preset | What it loads |
|---|---|
| `TwelveFactorPreset` | defaults, config file, profile file, env |
| `TwelveFactorLocal` | those four, plus `{stem}.local.{ext}` |
| `TwelveFactorConfDir` | those four, plus `conf.d` |
| `TwelveFactorDotenv` | those four, plus `.env` (`dotenv`) |
| `TwelveFactorCli` | those four, plus `--set` (`clap`) |
| `TwelveFactorDotenvCli` | those four, plus dotenv and cli |
| `InterpolatingPreset` | the twelve-factor list, with `${}` on opted-in fields |
| `SecretsPreset` | the twelve-factor list, with the std secret session (`secrets`) |
| `StrictPreset` | the twelve-factor list; unknown keys are errors, and so is an active profile defined nowhere |
| `TestConfigPreset` | defaults, an in-memory document, and env |
| `CoreConfigPreset` | the value-tree half: empty value, merge, serde, deny-secrets session |

New behavior is a new preset type. `TwelveFactorPreset`'s layer list stays put.
`#[version]` on a preset module is the fork's preset version, not the config file's
`$version`.
