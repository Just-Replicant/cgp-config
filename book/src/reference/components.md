# Components and providers

The facade prelude re-exports the component traits and the providers an application
names in `delegate_components!`. The ones a 0.1 app sets by hand:

| Component | Typical provider | Role |
|---|---|---|
| `ErrorTypeProviderComponent` | `UseType<ConfigError>` | the context's error |
| `ErrorRaiserComponent` | `RaiseFrom` | `From<ConfigError>` |
| `RootConfigMetaComponent` | `ConfigMetaField` | `Config::META` on the loader |
| `ConfigLoaderComponent` | `LoadLayered<Sources>` (from the preset) | the five-stage pipeline |
| `ConfigFileReaderComponent` | `ReadStdFile` or `ReadFromMap` | disk, or an in-memory map |
| `ConfigValidatorComponent` | `Nested`, or `AllOf<Product![... , Nested]>` | section rules |
| `SecretSessionComponent` | `NoSecretSession` or `OpenStdSession` | deny, or resolve |

`quick::load_from` uses a context that is `StdLoader` with `ReadFromMap` in place of
`ReadStdFile`. The context type is `#[doc(hidden)]`. Call `load_from`; do not construct
the context.

`ProjectSection<Path>` implements `FieldGetter`. `Path!(@config.http)` is
`config` then `http`. The base case, `Path` empty, returns the context itself.

A consumer of `#[cgp_getter]` or `delegate_components!` depends on `cgp-fork` directly.
The macros expand to `::cgp_fork`.
