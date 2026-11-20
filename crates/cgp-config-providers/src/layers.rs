//! Layer lists as types, and what a context can ask of them.
//!
//! A preset's `ConfigLoaderComponent` is `LoadLayered<L>` for a `Product![..]` list `L`
//! of source markers. The standard lists are named here ([`TwelveFactorLayers`] and its
//! variants) so a context, [`Bootstrap::layers`](crate::bootstrap::Bootstrap::layers),
//! and [`StdLoader`](crate::loader::StdLoader) can agree on one list: bootstrap accepts
//! `--set` only when the list has `CliArgs`, defaults the `.env` path only when it has
//! `DotEnv`, and the loader refuses bootstrap inputs no layer would read (instead of
//! dropping them silently).

use core::any::TypeId;

use cgp_config_core::markers::{
    CliArgs, ConfigDir, ConfigFile, Defaults, DotEnv, EnvDocument, EnvVars, InMemory, LocalFile,
    ProfileFile,
};
use cgp_fork::prelude::*;

/// A `Product![..]` list of source markers.
pub trait LayerList: 'static {
    /// Whether `Marker` is one of the layers.
    #[must_use]
    fn contains<Marker: 'static>() -> bool;
}

impl LayerList for Nil {
    fn contains<Marker: 'static>() -> bool {
        false
    }
}

impl<Head: 'static, Tail: LayerList> LayerList for Cons<Head, Tail> {
    fn contains<Marker: 'static>() -> bool {
        TypeId::of::<Head>() == TypeId::of::<Marker>() || Tail::contains::<Marker>()
    }
}

/// `TwelveFactorPreset`: `Defaults`, `ConfigFile`, `ProfileFile`, `EnvVars`.
pub type TwelveFactorLayers = Product![Defaults, ConfigFile, ProfileFile, EnvVars];

/// `TwelveFactorLocal`: … `ProfileFile`, `LocalFile`, `EnvVars`.
pub type TwelveFactorLocalLayers = Product![Defaults, ConfigFile, ProfileFile, LocalFile, EnvVars];

/// `TwelveFactorConfDir`: … `ConfigFile`, `ConfigDir`, `ProfileFile`, `EnvVars`.
pub type TwelveFactorConfDirLayers =
    Product![Defaults, ConfigFile, ConfigDir, ProfileFile, EnvVars];

/// `TwelveFactorCli`: … `EnvVars`, `CliArgs`.
pub type TwelveFactorCliLayers = Product![Defaults, ConfigFile, ProfileFile, EnvVars, CliArgs];

/// `TwelveFactorDotenv`: … `ProfileFile`, `DotEnv`, `EnvVars`.
pub type TwelveFactorDotenvLayers = Product![Defaults, ConfigFile, ProfileFile, DotEnv, EnvVars];

/// `TwelveFactorDotenvCli`: … `ProfileFile`, `DotEnv`, `EnvVars`, `CliArgs`.
pub type TwelveFactorDotenvCliLayers =
    Product![Defaults, ConfigFile, ProfileFile, DotEnv, EnvVars, CliArgs];

/// The twelve-factor layers with `{PREFIX}_DOCUMENT` between the files and env (feature
/// `json` wires its loader).
pub type TwelveFactorDocumentLayers =
    Product![Defaults, ConfigFile, ProfileFile, EnvDocument, EnvVars];

/// `TestConfigPreset`: `Defaults`, `InMemory`, `EnvVars`.
pub type TestLayers = Product![Defaults, InMemory, EnvVars];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership() {
        assert!(TwelveFactorLayers::contains::<EnvVars>());
        assert!(!TwelveFactorLayers::contains::<CliArgs>());
        assert!(TwelveFactorCliLayers::contains::<CliArgs>());
        assert!(TwelveFactorDotenvLayers::contains::<DotEnv>());
        assert!(!TestLayers::contains::<ConfigFile>());
    }
}
