//! # cgp-config-formats
//!
//! Document formats of [`cgp-config`](https://docs.rs/cgp-config).
//!
//! | Format | Feature | Parser | Extensions |
//! |---|---|---|---|
//! | [`Toml`] | `toml` (default) | [`parse_toml`], provider [`TomlFormat`] | `toml` |
//! | [`Json`] | `json` | [`parse_json`], provider [`JsonFormat`] | `json` |
//!
//! Every parser:
//!
//! * records the origin and the span (`line:column`) of every node in its `Meta`, so
//!   later errors read `config/app.toml:12:8`;
//! * rejects duplicate keys (TOML as a syntax error, JSON as `ConfigError::DuplicateKey`
//!   with both the path and the position);
//! * enforces `LoadLimits::max_depth` and `max_keys` (JSON while parsing; TOML after its
//!   own recursion guard);
//! * reports syntax errors as `ConfigError::Parse { origin, span, message }`.
//!
//! The same providers render (`CanRenderFormat`), always from a redacted copy of the tree.
//! [`EnabledFormats`] is the `UseDelegate` table of every compiled-in format.
//!
//! [`FormatKind`] picks a format from a file extension at run time, for sources that
//! accept whichever formats are compiled in ([`parse_document`]).
#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "json")]
mod json;
mod kind;
mod line_index;
mod providers;
#[cfg(feature = "toml")]
mod toml_doc;

#[cfg(feature = "json")]
pub use json::parse_json;
pub use kind::{FileFormat, FormatKind, Json, Toml, parse_document, render_document};
pub use line_index::LineIndex;
pub use providers::EnabledFormats;
#[cfg(feature = "json")]
pub use providers::JsonFormat;
#[cfg(feature = "toml")]
pub use providers::TomlFormat;
#[cfg(feature = "toml")]
pub use toml_doc::{parse_toml, render_toml};
