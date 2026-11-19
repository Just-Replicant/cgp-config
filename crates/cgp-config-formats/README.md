# cgp-config-formats

Document formats for [`cgp-config`](../cgp-config):

* **TOML** (feature `toml`, default): the `toml` crate's spanned document parser; every
  node keeps its file, line, and column; duplicate keys and invalid documents are
  `ConfigError::Parse` with `file:line:col`.
* **JSON** (feature `json`): a strict RFC 8259 parser written for configuration: spans on
  every node, duplicate keys rejected as `ConfigError::DuplicateKey`, depth and key limits
  enforced while parsing.
* Renderers for both, which write a **redacted** copy of the tree so a dump never
  contains a secret.

The parsers are plain functions (`parse_toml`, `parse_json`, `parse_document`) and CGP
providers (`TomlFormat`, `JsonFormat`) of `CanParseFormat<Fmt>` and `CanRenderFormat<Fmt>`;
`EnabledFormats` is the `UseDelegate` table of every compiled-in format.

`#![no_std]` with `alloc`. Applications depend on the `cgp-config` facade, which
re-exports this crate.

License: AGPL-3.0-or-later.
