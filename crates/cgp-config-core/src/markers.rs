//! The built-in source markers.
//!
//! A marker is a zero-sized type that names one layer. Contexts list markers in
//! `LoadLayered<Product![..]>`; `CanLoadSource<Marker>` and `CanPrepareLayer<Marker>` are
//! dispatched per marker with `UseDelegate` tables.
//!
//! The markers live here, below every provider crate, so that preset tables such as
//! `cgp_config_value::preset::DefaultPreparers` can name all of them. The loaders that
//! serve them arrive with the source providers; until then a context supplies its own
//! `SourceLoader<Marker>` provider.
//!
//! | Marker | Layer | Preparer in `DefaultPreparers` |
//! |---|---|---|
//! | [`Defaults`] | const defaults of the config type | `IdentityLayer` |
//! | [`ConfigFile`] | the main config file | `PrepareFileLayer` |
//! | [`ConfigDir`] | `conf.d/*` next to the main file | `PrepareFileLayer` |
//! | [`ProfileFile`] | `config.{profile}.toml` | `PrepareFileLayer` |
//! | [`LocalFile`] | `{stem}.local.toml` | `PrepareFileLayer` |
//! | [`Embedded`] | a document compiled into the binary | `PrepareFileLayer` |
//! | [`EnvVars`] | `{PREFIX}__*` environment variables | `IdentityLayer` |
//! | [`EnvDocument`] | `{PREFIX}_DOCUMENT`, one JSON object | `IdentityLayer` |
//! | [`DotEnv`] | a `.env` file | `IdentityLayer` |
//! | [`CliArgs`] | parsed command-line overrides | `IdentityLayer` |
//! | [`InMemory`] | a tree held by the context | `IdentityLayer` |
//! | [`DynSources`] | boxed dynamic sources | `IdentityLayer` |

macro_rules! markers {
    ($($(#[$doc:meta])* $name:ident,)*) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
        pub struct $name;
    )*};
}

markers! {
    /// Const defaults from the config type's metadata.
    Defaults,
    /// The main config file (explicit path or the first search-path hit).
    ConfigFile,
    /// Every `conf.d/*` file next to the main file, in sorted order.
    ConfigDir,
    /// The profile file beside the main file (`config.{profile}.toml`).
    ProfileFile,
    /// The local override file beside the main file (`{stem}.local.toml`).
    LocalFile,
    /// A document compiled into the binary (`include_str!`).
    Embedded,
    /// Environment variables under the application prefix.
    EnvVars,
    /// One environment variable holding a whole JSON document.
    EnvDocument,
    /// A dotenv file.
    DotEnv,
    /// Command-line overrides the application already parsed.
    CliArgs,
    /// A tree held by the context.
    InMemory,
    /// Boxed dynamic sources (`HasDynSources`).
    DynSources,
}
