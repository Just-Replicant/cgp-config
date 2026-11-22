# cgp-config-secrets

`Secret<T>` and `LazySecret<T>`, plus the `env:`, `file:`, and `docker:` resolvers of
[`cgp-config`](../cgp-config) (design plan phase P4).

Applications depend on the `cgp-config` facade with feature `secrets`, which re-exports
this crate. `Debug`, `Display`, and dumps print `[REDACTED]`. Secret bytes live only in
`Secret<T>` after resolution and are zeroized when the last clone is dropped.

License: AGPL-3.0-or-later.
