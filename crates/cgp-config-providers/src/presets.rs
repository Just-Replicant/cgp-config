//! The standard presets: [`TwelveFactorPreset`] and its children.
//!
//! Every preset extends `CoreConfigPreset` (value tree, merge, preparation, warn on
//! unknown keys, no interpolation, no validation, `DenySecrets`) and adds:
//!
//! | Component | Provider |
//! |---|---|
//! | `SourceLoaderComponent` | `UseDelegate<`[`StdSources`]`>` |
//! | `ConfigLoaderComponent` | `LoadLayered<Product![..]>` with the preset's layer list |
//! | `ConfigFileReaderComponent` | [`ReadStdFile`] |
//! | `FormatParserComponent`, `FormatRendererComponent` | `UseDelegate<EnabledFormats>` |
//! | `LoggerComponent` | [`NoopLogger`] (wire `TracingLogger` to see source events) |
//!
//! | Preset | Layers (low → high) | Feature |
//! |---|---|---|
//! | [`TwelveFactorPreset`] | `Defaults`, `ConfigFile`, `ProfileFile`, `EnvVars` | — |
//! | `TwelveFactorDotenv` | … `ProfileFile`, `DotEnv`, `EnvVars` | `dotenv` |
//! | `TwelveFactorCli` | … `EnvVars`, `CliArgs` | `clap` |
//! | `TwelveFactorDotenvCli` | … `ProfileFile`, `DotEnv`, `EnvVars`, `CliArgs` | `dotenv` + `clap` |
//! | [`TwelveFactorLocal`] | … `ProfileFile`, `LocalFile`, `EnvVars` (dev and test only) | — |
//! | [`TwelveFactorConfDir`] | … `ConfigFile`, `ConfigDir`, `ProfileFile`, `EnvVars` | — |
//! | `InterpolatingPreset` | as `TwelveFactorPreset`, with `InterpolateOptedIn` | `interpolate` |
//! | [`TestConfigPreset`] | `Defaults`, `InMemory`, `EnvVars` (no file IO) | — |
//!
//! The context supplies the fields the providers read: `env_snapshot`, `env_prefix`,
//! `active_profile`, `config_location`, `load_report` (and `dotenv_path`,
//! `cli_overrides`, `memory_layer` for the presets that read them), plus its error type.
//! [`StdLoader`](crate::loader::StdLoader) is a ready-made context with all of them.
//! A context with a derived root section adds `RootConfigMetaComponent:
//! UseConfigMeta<AppConfig>` so defaults, env mapping, and schema rules apply.
//!
//! Behavior changes ship as new preset types; `#[version]` is the fork's preset version.
//! (`cgp_preset!` accepts no doc attributes, so the presets are documented here.)

#![allow(missing_docs)]

#[doc(hidden)]
pub mod __entries {
    pub use cgp_config_core::components::*;
    pub use cgp_config_core::{
        ConfigLoaderComponent, FormatParserComponent, FormatRendererComponent, LoadLayered,
        NoopLogger, SourceLoaderComponent,
    };
    pub use cgp_config_formats::EnabledFormats;
    pub use cgp_config_value::CoreConfigPreset;
    pub use cgp_config_value::preset::__entries::*;
    pub use cgp_fork::prelude::{LoggerComponent, UseDelegate};

    pub use crate::presets::StdSources;
    pub use crate::read::{ConfigFileReaderComponent, ReadStdFile};
}

use cgp_config_core::markers::{
    CliArgs, ConfigDir, ConfigFile, Defaults, DotEnv, DynSources, Embedded, EnvDocument, EnvVars,
    InMemory, LocalFile, ProfileFile,
};
use cgp_config_value::LoadDefaults;
use cgp_fork::prelude::*;

#[allow(clippy::wildcard_imports)]
use self::__entries::*;
use crate::env::LoadEnvVars;
use crate::file::{LoadConfDir, LoadFileAuto, LoadLocalFile, LoadProfileFile};
use crate::sources::{LoadCliOverrides, LoadDynSources, LoadFromField};

/// The source table of every standard preset: one provider per built-in marker.
/// Markers a preset's layer list does not name are never asked for, so their field
/// requirements do not apply.
///
/// | Marker | Provider |
/// |---|---|
/// | `Defaults` | `LoadDefaults` (from the root `ConfigMeta`) |
/// | `ConfigFile` | `LoadFileAuto` (format by extension) |
/// | `ProfileFile`, `LocalFile`, `ConfigDir` | `LoadProfileFile`, `LoadLocalFile`, `LoadConfDir` |
/// | `EnvVars` | `LoadEnvVars` |
/// | `EnvDocument` | `LoadEnvDocument` (feature `json`) |
/// | `DotEnv` | `LoadDotenv` (feature `dotenv`) |
/// | `CliArgs` | `LoadCliOverrides` |
/// | `InMemory` | `LoadFromField<Symbol!("memory_layer")>` |
/// | `DynSources` | `LoadDynSources` |
///
/// `Embedded` has no entry: the format is the context's choice (`LoadEmbedded<Toml>`).
pub struct StdSources;

delegate_components! {
    #[unchecked]
    StdSources {
        Defaults: LoadDefaults,
        ConfigFile: LoadFileAuto,
        ProfileFile: LoadProfileFile,
        LocalFile: LoadLocalFile,
        ConfigDir: LoadConfDir,
        EnvVars: LoadEnvVars,
        CliArgs: LoadCliOverrides,
        InMemory: LoadFromField<Symbol!("memory_layer")>,
        DynSources: LoadDynSources,
    }
}

#[cfg(feature = "json")]
delegate_components! {
    #[unchecked]
    StdSources {
        EnvDocument: crate::sources::LoadEnvDocument,
    }
}

#[cfg(feature = "dotenv")]
delegate_components! {
    #[unchecked]
    StdSources {
        DotEnv: crate::dotenv::LoadDotenv,
    }
}

// Silences the unused-marker imports when a feature is off; they document the table.
#[allow(dead_code)]
type _Unwired = (DotEnv, EnvDocument, Embedded);

cgp_preset! {
    #[version(1)]
    TwelveFactorPreset: CoreConfigPreset {
        SourceLoaderComponent: UseDelegate<StdSources>,
        ConfigLoaderComponent: LoadLayered<Product![Defaults, ConfigFile, ProfileFile, EnvVars]>,
        ConfigFileReaderComponent: ReadStdFile,
        FormatParserComponent: UseDelegate<EnabledFormats>,
        FormatRendererComponent: UseDelegate<EnabledFormats>,
        LoggerComponent: NoopLogger,
    }
}

cgp_preset! {
    #[version(1)]
    TwelveFactorLocal: TwelveFactorPreset {
        override ConfigLoaderComponent:
            LoadLayered<Product![Defaults, ConfigFile, ProfileFile, LocalFile, EnvVars]>,
    }
}

cgp_preset! {
    #[version(1)]
    TwelveFactorConfDir: TwelveFactorPreset {
        override ConfigLoaderComponent:
            LoadLayered<Product![Defaults, ConfigFile, ConfigDir, ProfileFile, EnvVars]>,
    }
}

cgp_preset! {
    #[version(1)]
    TestConfigPreset: TwelveFactorPreset {
        override ConfigLoaderComponent: LoadLayered<Product![Defaults, InMemory, EnvVars]>,
    }
}

#[cfg(feature = "dotenv")]
pub use self::dotenv_presets::*;

#[cfg(feature = "dotenv")]
mod dotenv_presets {
    #![allow(missing_docs)]

    use cgp_config_core::markers::{ConfigFile, Defaults, DotEnv, EnvVars, ProfileFile};
    use cgp_fork::prelude::*;

    #[allow(clippy::wildcard_imports)]
    use super::__entries::*;
    use super::TwelveFactorPreset;

    cgp_preset! {
        #[version(1)]
        TwelveFactorDotenv: TwelveFactorPreset {
            override ConfigLoaderComponent:
                LoadLayered<Product![Defaults, ConfigFile, ProfileFile, DotEnv, EnvVars]>,
        }
    }
}

#[cfg(feature = "clap")]
pub use self::cli_presets::*;

#[cfg(feature = "clap")]
mod cli_presets {
    #![allow(missing_docs)]

    use cgp_config_core::markers::{CliArgs, ConfigFile, Defaults, EnvVars, ProfileFile};
    use cgp_fork::prelude::*;

    #[allow(clippy::wildcard_imports)]
    use super::__entries::*;
    use super::TwelveFactorPreset;

    cgp_preset! {
        #[version(1)]
        TwelveFactorCli: TwelveFactorPreset {
            override ConfigLoaderComponent:
                LoadLayered<Product![Defaults, ConfigFile, ProfileFile, EnvVars, CliArgs]>,
        }
    }
}

#[cfg(all(feature = "dotenv", feature = "clap"))]
pub use self::dotenv_cli_presets::*;

#[cfg(all(feature = "dotenv", feature = "clap"))]
mod dotenv_cli_presets {
    #![allow(missing_docs)]

    use cgp_config_core::markers::{CliArgs, ConfigFile, Defaults, DotEnv, EnvVars, ProfileFile};
    use cgp_fork::prelude::*;

    #[allow(clippy::wildcard_imports)]
    use super::__entries::*;
    use super::TwelveFactorPreset;

    cgp_preset! {
        #[version(1)]
        TwelveFactorDotenvCli: TwelveFactorPreset {
            override ConfigLoaderComponent:
                LoadLayered<Product![Defaults, ConfigFile, ProfileFile, DotEnv, EnvVars, CliArgs]>,
        }
    }
}

#[cfg(feature = "interpolate")]
pub use self::interpolating::*;

#[cfg(feature = "interpolate")]
mod interpolating {
    #![allow(missing_docs)]

    use cgp_config_core::InterpolatorComponent;
    use cgp_config_value::InterpolateOptedIn;
    use cgp_fork::prelude::*;

    #[allow(clippy::wildcard_imports)]
    use super::__entries::*;
    use super::TwelveFactorPreset;

    cgp_preset! {
        #[version(1)]
        InterpolatingPreset: TwelveFactorPreset {
            override InterpolatorComponent: InterpolateOptedIn,
        }
    }
}
