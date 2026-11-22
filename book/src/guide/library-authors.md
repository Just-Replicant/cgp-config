# Library authors

A library publishes a section. It does not load files and it does not read the process
environment.

```toml
[dependencies]
cgp-config = { version = "0.1.0-alpha.1", default-features = false, features = ["derive"] }
cgp-fork = { version = "1.1", default-features = false }
serde = { version = "1", default-features = false, features = ["derive", "alloc"] }
```

`#[cgp_getter]` expands to `::cgp_fork`, so the library names `cgp-fork` itself.

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, cgp_config::ConfigSection)]
#[config(section = "http", getter)]
pub struct HttpConfig {
    #[config(default = 8080)]
    pub port: u16,
    #[config(default = "0.0.0.0")]
    pub host: String,
}
```

`#[config(getter)]` emits `HasHttpConfig` and `fn http_config(&self) -> &HttpConfig`.
`HttpConfig` and a struct named `Http` both produce that trait: a trailing `Config` is
stripped when the remainder starts with an uppercase letter. The component the
application wires is `HttpConfigGetterComponent`, generated in this module. Re-export it
by leaving the struct public; the component is public next to the trait.

`#[config(section = "http")]` is the default document key. The application can mount the
same type more than once. Env names are `{PREFIX}__{MOUNT}__{FIELD}`. `env = "PORT"`
replaces the field segment. `env_raw = "PORT"` is the process-global name and must be
unique in the root config. Do not give one section both a field key `port` and
`env = "PORT"` on another field: env segments fold case, so those names collide.

`#[config(partial)]` is also opt-in. It emits `{Name}Partial` with `Option` fields (an
existing `Option<T>` is not wrapped again) and `FinalizeOptionalAll`. Const defaults
fill what was left unset. Required fields that are still unset are reported together.
A `#[serde(skip)]` field stays on the real struct, is filled with `Default`, and is
absent from the partial and from `ConfigMeta`.

The default derive emits `ConfigSection` and `ValidateSections`. It does not emit
`HasField` or any CGP wiring.
