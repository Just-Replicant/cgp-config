use cgp_fork::prelude::*;

use crate::{
    CanCollectLayers, CanDeserializeConfig, CanInterpolate, CanResolveSecrets, CanValidateConfig,
    ConfigLoader, ConfigLoaderComponent, HasEmptyConfigValue,
};

/// Loads `Config` from the sources in `Sources` (a `Product![..]` of markers), running the
/// normative pipeline:
///
/// 1. collect the layers in list order (load, prepare, merge),
/// 2. interpolate opted-in strings,
/// 3. deserialize,
/// 4. resolve secrets,
/// 5. validate.
///
/// The same stages serve every reload, so a value that loads at startup loads the same way
/// later.
pub struct LoadLayered<Sources>(PhantomData<Sources>);

#[cgp_impl(LoadLayered<Sources>)]
impl<Sources, Config> ConfigLoader<Config>
where
    Self: HasEmptyConfigValue
        + CanCollectLayers<Sources>
        + CanInterpolate<Config>
        + CanDeserializeConfig<Config>
        + CanResolveSecrets<Config>
        + CanValidateConfig<Config>,
{
    fn load_config(&self) -> Result<Config, Self::Error> {
        let tree = self.collect_layers(self.empty_config_value(), PhantomData::<Sources>)?;
        let tree = self.interpolate(tree)?;
        let mut config = self.deserialize_config(tree)?;
        self.resolve_secrets(&mut config)?;
        self.validate_config(&config)?;
        Ok(config)
    }
}
