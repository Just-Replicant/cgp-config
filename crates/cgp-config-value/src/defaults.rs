//! The `Defaults` source: const defaults from `ConfigMeta`.

use cgp_config_core::markers::Defaults;
use cgp_config_core::{
    ConfigSection, HasConfigValueType, HasRootConfigMeta, SourceLoader, SourceLoaderComponent,
};
use cgp_fork::prelude::*;

use crate::ConfigValue;
use crate::schema::defaults_tree;

/// Loads the `Defaults` layer from the context's root schema (`HasRootConfigMeta`):
/// every `#[config(default = ...)]` at its document key, stamped `Origin::Defaults`.
/// Absent (`None`) without a root schema or without any default.
pub struct LoadDefaults;

#[cgp_impl(LoadDefaults)]
impl SourceLoader<Defaults>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue> + HasErrorType + HasRootConfigMeta,
{
    fn load_source(
        &self,
        _source: PhantomData<Defaults>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        Ok(self.root_config_meta().and_then(non_empty_defaults))
    }
}

/// Loads the `Defaults` layer of `Config` whatever the context's root schema is.
pub struct LoadDefaultsOf<Config>(PhantomData<Config>);

#[cgp_impl(LoadDefaultsOf<Config>)]
impl<Config> SourceLoader<Defaults>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue> + HasErrorType,
    Config: ConfigSection,
{
    fn load_source(
        &self,
        _source: PhantomData<Defaults>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        Ok(non_empty_defaults(Config::META))
    }
}

fn non_empty_defaults(meta: &'static cgp_config_core::ConfigMeta) -> Option<ConfigValue> {
    let tree = defaults_tree(meta);
    tree.as_map()
        .is_some_and(|map| !map.is_empty())
        .then_some(tree)
}
