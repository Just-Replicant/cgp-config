use cgp_fork::prelude::*;

use crate::{
    CanCollectLayers, CanDeserializeConfig, CanInterpolate, CanResolveSecrets, CanValidateConfig,
    ConfigLoader, ConfigLoaderComponent, HasEmptyConfigValue,
};
#[cfg(feature = "alloc")]
use crate::{CanOpenSecretSession, CanReportWarning};

/// Loads `Config` from the sources in `Sources` (a `Product![..]` of markers), running the
/// normative pipeline:
///
/// 0. clear the warnings of the previous load (`CanReportWarning::clear_warnings`, with
///    the `alloc` feature; wire `IgnoreWarnings` for a context without a report),
/// 1. collect the layers in list order (load, prepare, merge),
/// 2. interpolate opted-in strings,
/// 3. open the secret session, then deserialize (`Secret<T>` reads the session),
/// 4. resolve secrets (a no-op once the session has done the work),
/// 5. validate.
///
/// The same stages serve every reload, so a value that loads at startup loads the same way
/// later.
pub struct LoadLayered<Sources>(PhantomData<Sources>);

#[cfg(feature = "alloc")]
#[cgp_impl(LoadLayered<Sources>)]
impl<Sources, Config> ConfigLoader<Config>
where
    Self: HasEmptyConfigValue
        + CanReportWarning
        + CanOpenSecretSession
        + CanCollectLayers<Sources>
        + CanInterpolate<Config>
        + CanDeserializeConfig<Config>
        + CanResolveSecrets<Config>
        + CanValidateConfig<Config>,
{
    fn load_config(&self) -> Result<Config, Self::Error> {
        self.clear_warnings();
        run_stages::<Self, Sources, Config>(self)
    }
}

/// Without `alloc` there is no warning type and so no report to clear.
#[cfg(not(feature = "alloc"))]
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
        run_stages::<Self, Sources, Config>(self)
    }
}

/// Stages 1 to 5, shared by both feature sets. With `alloc` the secret session is open
/// for the deserialize call.
#[cfg(feature = "alloc")]
fn run_stages<Context, Sources, Config>(context: &Context) -> Result<Config, Context::Error>
where
    Context: HasEmptyConfigValue
        + CanOpenSecretSession
        + CanCollectLayers<Sources>
        + CanInterpolate<Config>
        + CanDeserializeConfig<Config>
        + CanResolveSecrets<Config>
        + CanValidateConfig<Config>,
{
    let tree = context.collect_layers(context.empty_config_value(), PhantomData::<Sources>)?;
    let tree = context.interpolate(tree)?;
    let _session = context.open_secret_session();
    let mut config = context.deserialize_config(tree)?;
    context.resolve_secrets(&mut config)?;
    context.validate_config(&config)?;
    Ok(config)
}

/// Without `alloc` there is no secret session.
#[cfg(not(feature = "alloc"))]
fn run_stages<Context, Sources, Config>(context: &Context) -> Result<Config, Context::Error>
where
    Context: HasEmptyConfigValue
        + CanCollectLayers<Sources>
        + CanInterpolate<Config>
        + CanDeserializeConfig<Config>
        + CanResolveSecrets<Config>
        + CanValidateConfig<Config>,
{
    let tree = context.collect_layers(context.empty_config_value(), PhantomData::<Sources>)?;
    let tree = context.interpolate(tree)?;
    let mut config = context.deserialize_config(tree)?;
    context.resolve_secrets(&mut config)?;
    context.validate_config(&config)?;
    Ok(config)
}
