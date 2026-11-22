# `config::secrets_denied`

The context does not resolve secrets, and the config contains a reference. This is the
default session (`NoSecretSession` / `DenySecrets`): `quick::load`, `StdLoader` on
`TwelveFactorPreset`, and `from_toml_str`. A literal secret still loads.

Wire `SecretsPreset`, or set `SecretSessionComponent: OpenStdSession`, to resolve
`env:`, `file:`, and `docker:` references.
