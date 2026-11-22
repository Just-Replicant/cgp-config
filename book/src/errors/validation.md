# `config::validation`

One or more validators rejected the loaded config. `Nested` walks each
`#[config(nested)]` section and prefixes the path with the field key. `AllOf` runs
every rule and keeps every failure.

A `ValidationIssue` defaults to this code. A rule can set its own code string; the
parent error's `code()` stays `config::validation`.
