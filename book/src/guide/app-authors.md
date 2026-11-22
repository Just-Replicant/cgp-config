# Application authors

The application owns the root config, the loader, and the request context.

Mount each library section with `nested` and, when the document key differs from the
field name, `mount`:

```rust
#[derive(Debug, Clone, serde::Deserialize, ConfigSection, HasField)]
pub struct AppConfig {
    #[config(nested, mount = "http")]
    pub http: HttpConfig,
    #[config(nested, mount = "db")]
    pub db: DbConfig,
    #[config(default = "demo")]
    pub name: String,
}
```

`nested` is what makes the field a section: defaults, env paths, and validation walk
into `HttpConfig::META`. `mount` is the key in documents and in env names. Two fields
whose resolved mount is the same are a compile error. The same section type may be
mounted twice under two mounts (`api_http` and `admin_http`).

A request holds `Arc<AppConfig>` for the length of the request. The getters borrow from
that `Arc`. `Path!` needs a leading `@`, and `#[cgp_getter]` only implements `UseField`
and `WithProvider`:

```rust
#[derive(HasField)]
pub struct Request {
    pub config: Arc<AppConfig>,
}

delegate_components! {
    Request {
        HttpConfigGetterComponent:
            WithProvider<ProjectSection<Path!(@config.http)>>,
        DbConfigGetterComponent:
            WithProvider<ProjectSection<Path!(@config.db)>>,
    }
}
```

`ProjectSection` walks `HasField` and returns `&HttpConfig`. It does not borrow through
`ArcSwap`. Import `HttpConfigGetterComponent` from the crate that derived the getter.

`UseField<Symbol!("http")>` is the wiring when the context stores the section as a field
of that name, with no extra hop.

The long-lived process will hold a `ConfigHandle` in 0.2 and copy an `Arc<AppConfig>`
into `Request` at the start of each request. 0.1 has no reload handle.

The worked crate is `examples/multi-crate-workspace`: `example-http-server` and
`example-db-pool` are `no_std` libraries, and `example-multi-crate-app` loads both
sections with `quick::load_from`.
