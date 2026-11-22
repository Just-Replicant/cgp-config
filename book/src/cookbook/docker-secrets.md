# Docker secrets

Feature `secrets`. Wire a context with `SecretsPreset` (or override
`SecretSessionComponent` on your own table). `StdSecretRoots` includes
`/run/secrets` and `CREDENTIALS_DIRECTORY`.

A field

```rust
pub password: Secret<String>,
```

with the document value `docker:db_password` reads `/run/secrets/db_password` (or the
credentials directory of the same name) while the field deserializes. The file bytes
are the secret. They are not copied into `ConfigValue`, `LoadReport`, or `explain`.

`file:relative-name` is read from the same roots. A path that escapes the roots is
`config::secret_denied`. `env:NAME` reads that variable from the snapshot captured at
bootstrap, not from a later `std::env` call.

`quick::load` and `from_toml_str` do not open the session. A docker reference there
fails with `config::secrets_denied`.
