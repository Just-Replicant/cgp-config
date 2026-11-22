# Secrets

Feature `secrets` on the facade. A library field is either a literal or a reference:

```rust
#[config(secret)]
pub token: Option<Secret<String>>,
```

The derive also marks a field secret when its type is `Secret` or `LazySecret` (after
`Option`, `Vec`, `Box`, `Arc`, or `Rc`).

A reference stays in the value tree as text (`env:TOKEN`, `file:db_password`,
`docker:db_password`) with `Meta::secret`. The bytes are resolved while `Secret<T>`
deserializes, from a thread-local session the loader installs for that load. They are
zeroized when the last owner drops. `Debug` and `Display` print `[REDACTED]`.

`SecretsPreset` turns resolution on. It extends `TwelveFactorPreset` and wires
`OpenStdSession`, the `env` / `file` / `docker` schemes, and `StdSecretRoots`
(`CREDENTIALS_DIRECTORY`, the config directory, `/run/secrets`). `StdLoader` and
`quick::load` stay on `TwelveFactorPreset`, so a reference there is
`config::secrets_denied`. `from_toml_str` keeps the deny session too: a literal secret
loads, a reference does not.

`LazySecret<T>` does not resolve during load. The first `expose_secret` does.

A group- or world-readable file that holds a secret reference, or a schema secret field
with a value, warns `config::warn::secret_world_readable`. The diagnostic feature
`miette` suppresses the source snippet on a secret span.

See [Docker secrets](../cookbook/docker-secrets.md).
