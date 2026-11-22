# Attributes

`#[derive(ConfigSection)]` reads `#[config(...)]` and the serde keys that decide field
names (`rename`, `rename_all`, `skip`, `default`, `deny_unknown_fields`). Misuse is a
compile error, and every error on the struct is reported together.

On the struct: `section`, `getter`, `partial`, `version`, `env_prefix`, `deny_unknown`,
`rename_all`, `crate`. `migrate` is reserved for 0.2.

On a field: `default`, `env`, `env_raw`, `secret`, `alias`, `deprecated`, `reload`
(`live` or `restart`), `merge` (`replace`, `append`, `deep`), `interpolate`,
`parse = "duration" | "bytes"`, `nested`, `flatten`, `mount`, `skip`.

`nested` marks a section (`T`, `Option<T>`, `Vec<T>`, or a map of `T`). `mount` is the
document and env key when it differs from serde's. A mount on a section is written
`#[config(nested, mount = "http")]`. `flatten` lifts the nested keys onto the parent;
use it instead of `serde(flatten)`.

`default` is a const: bool, integer, float, string, or an array of those. A duration
default is a string with a unit (`"30s"`). `#[serde(default)]` uses `Default`.

`skip` requires `#[serde(skip)]` (or `skip_deserializing`) as well.

Keys starting with `$` are reserved: `$version`, `$profiles`, `$include`.

The derive's module docs (`cgp_config::derive`) are the attribute table with a doctest.
