# `config::unknown`

Keys the struct does not declare, when unknown keys are errors: `DenyUnknownKeys` on
the loader, or `#[config(deny_unknown)]` / `#[serde(deny_unknown_fields)]` on the
section. Every unknown key of the load is reported at once, in document order.

With the default preset the same keys are `config::warn::unknown_key` and the load
succeeds.
