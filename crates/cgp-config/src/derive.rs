//! `#[derive(ConfigSection)]`: the attribute reference.
//!
//! The derive reads `#[config(...)]`, the `#[serde(...)]` keys that decide field keys
//! (`rename`, `rename_all`, `skip`, `default`, `deny_unknown_fields`), and doc comments,
//! and emits:
//!
//! * `impl ConfigSection` with `const META: &'static ConfigMeta`. Loaders read it for
//!   defaults (the `Defaults` layer), env names, aliases, parsers, `flatten`, per-section
//!   `deny_unknown`, and to report every missing field at once;
//! * `impl ValidateSections<Context>`, which runs the context's validator on each nested
//!   section (and each element of a nested list or map) and aggregates the errors with
//!   their paths. Wire it with `ConfigValidatorComponent: Nested`, or combine it with a
//!   rule: `AllOf<Product![ValidateFn<MyRule>, Nested]>`.
//!
//! Serde stays in charge of deserialization: derive `serde::Deserialize` as usual.
//! Misuse is a compile error at the attribute, all of a struct's errors at once.
//!
//! # On the struct
//!
//! | Attribute | Meaning |
//! |---|---|
//! | `section = "http"` | the section's default key |
//! | `getter` | emit `Has{Name}Config` (`#[cgp_getter]`). `HttpConfig` and `Http` both emit `HasHttpConfig` |
//! | `partial` | emit `{Name}Partial` and `FinalizeOptionalAll` |
//! | `version = 2` | the schema version (default 1) |
//! | `env_prefix = "APP"` | the default env prefix of a root config |
//! | `deny_unknown` | unknown keys inside this section fail the load (also implied by `#[serde(deny_unknown_fields)]`) |
//! | `rename_all = "camelCase"` | as serde's; serde's own `rename_all` is read automatically |
//! | `crate = "path"` | the facade's path when it is renamed (default `::cgp_config`) |
//! | `migrate(...)` | reserved: schema migrations arrive in 0.2 |
//!
//! # On a field
//!
//! | Attribute | Meaning |
//! |---|---|
//! | `default = 8080` | a const default: bool, integer, float, string, or an array of them |
//! | `env = "PORT"` | the env segment for this field (replaces its key in `{PREFIX}__...`) |
//! | `env_raw = "PORT"` | an exact, unprefixed env name; a prefixed variable wins over it |
//! | `secret` | never echoed in errors, reports, or dumps |
//! | `alias = "old"` | an old key, accepted with a `DeprecatedKey` warning (repeatable) |
//! | `deprecated = "note"` | the field itself is deprecated (warns when set) |
//! | `reload = "restart"` | a change needs a restart (used by reload in 0.2) |
//! | `merge = "replace"` / `"append"` / `"deep"` | this field's merge policy (`KeyedMerge`) |
//! | `interpolate` | `${VAR}` / `${path}` in this field's strings is expanded (`InterpolateOptedIn`); other fields keep `${` literally |
//! | `parse = "duration"` | `"30s"`, `"1m30s"`, `"250ms"` into a `Duration`; a bare number is rejected |
//! | `parse = "bytes"` | `"10MiB"`, `"1KB"`, `512` into a byte count |
//! | `nested` | the field is a section (`T`, `Option<T>`, `Vec<T>`, or a map of `T`) |
//! | `flatten` | a nested section whose keys are written on the parent (instead of `serde(flatten)`) |
//! | `mount = "key"` | the key in documents and env names, when it differs from serde's. Two fields with the same mount are a compile error |
//! | `skip` | with `#[serde(skip)]`: the field is not configuration |
//!
//! Keys starting with `$` are reserved (`$version`, `$profiles`, `$include`).
//!
//! # Example
//!
//! ```
//! use cgp_config::prelude::*;
//!
//! #[derive(Debug, serde::Deserialize, ConfigSection)]
//! #[serde(rename_all = "camelCase")]
//! #[config(section = "db", deny_unknown)]
//! struct Database {
//!     /// Connection string.
//!     #[config(secret, env = "URL")]
//!     url: String,
//!     #[config(default = 10, alias = "pool")]
//!     max_connections: u32,
//!     #[config(default = "10MiB", parse = "bytes")]
//!     buffer: u64,
//! }
//!
//! let meta = Database::META;
//! assert_eq!(meta.section, Some("db"));
//! assert!(meta.field("url").unwrap().secret);
//! assert_eq!(meta.field("maxConnections").unwrap().aliases, ["pool"]);
//!
//! let db: Database = cgp_config::from_toml_str("url = \"pg://x\"\npool = 4\n")?;
//! assert_eq!((db.max_connections, db.buffer), (4, 10 * 1024 * 1024));
//!
//! let error = cgp_config::from_toml_str::<Database>("max_connections = 4\n").unwrap_err();
//! assert!(error.to_string().contains("url"), "{error}");
//! # Ok::<(), ConfigError>(())
//! ```
