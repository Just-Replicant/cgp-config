//! The root [`ConfigMeta`] of a loader: which config type it loads, for the providers
//! that need the schema (env-key mapping, defaults, aliases, durations, secrets,
//! interpolation opt-in, per-section `deny_unknown`).

use cgp_fork::prelude::*;

use crate::{ConfigMeta, ConfigSection};

/// The metadata of the config type a context loads, or `None` for a schema-less loader.
///
/// A component (not a getter) so a preset supplies `NoConfigMeta` and a context names
/// its type with `RootConfigMetaComponent: UseConfigMeta<AppConfig>`.
#[cgp_component {
    name: RootConfigMetaComponent,
    provider: RootConfigMetaGetter,
}]
pub trait HasRootConfigMeta {
    /// The root metadata.
    fn root_config_meta(&self) -> Option<&'static ConfigMeta>;
}

/// No schema: env keys map by splitting on `__` only, and nothing is normalized. The
/// preset provider.
pub struct NoConfigMeta;

#[cgp_impl(NoConfigMeta: RootConfigMetaComponent)]
impl RootConfigMetaGetter {
    fn root_config_meta(&self) -> Option<&'static ConfigMeta> {
        None
    }
}

/// The schema of `Config` (a `#[derive(ConfigSection)]` type).
pub struct UseConfigMeta<Config>(PhantomData<Config>);

#[cgp_impl(UseConfigMeta<Config>: RootConfigMetaComponent)]
impl<Config> RootConfigMetaGetter
where
    Config: ConfigSection,
{
    fn root_config_meta(&self) -> Option<&'static ConfigMeta> {
        Some(Config::META)
    }
}

/// Reads a `config_meta: Option<&'static ConfigMeta>` field, for loaders whose type is
/// chosen at run time.
pub struct ConfigMetaField;

#[cgp_impl(ConfigMetaField: RootConfigMetaComponent)]
impl RootConfigMetaGetter
where
    Self: HasField<Symbol!("config_meta"), Value = Option<&'static ConfigMeta>>,
{
    fn root_config_meta(&self) -> Option<&'static ConfigMeta> {
        *self.get_field(PhantomData::<Symbol!("config_meta")>)
    }
}
