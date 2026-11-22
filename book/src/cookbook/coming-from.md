# Coming from config-rs, figment, or confique

The env separator is `__`. `APP__HTTP__PORT` is the field `port` of the section mounted
at `http`, with prefix `APP`. A single `_` is not a separator. Segments fold to
`snake_case`. File keys stay case-sensitive. Two fields that fold to the same env
segment are `config::ambiguous_env`.

Unknown keys warn. They do not fail the load unless the section is
`#[config(deny_unknown)]` or the loader uses `DenyUnknownKeys`.

This crate does not mutate the process environment. Bootstrap copies it once into an
`EnvSnapshot`. A `.env` file, when the `dotenv` feature is on, is a layer of that
snapshot. It is not `dotenv()`.

Defaults live on the struct (`#[config(default = 8080)]`), not in a separate builder
chain. `"30s"` is a duration. A bare `30` is rejected. `"10MiB"` is a byte size. A bare
number of bytes is accepted.

There is no `set` of the process environment after load, and no global `Config::load`
that libraries call. Libraries export a section. The application calls `quick::load` or
builds a `StdLoader`.
