use cgp_fork::prelude::*;

/// Loads a complete, validated config. This is what applications call.
///
/// `LoadLayered<Sources>` runs the whole pipeline: collect, interpolate, deserialize,
/// resolve secrets, validate.
#[cgp_component(ConfigLoader)]
pub trait CanLoadConfig<Config>: HasErrorType {
    /// Loads the config.
    ///
    /// # Errors
    ///
    /// The first failing stage's error.
    fn load_config(&self) -> Result<Config, Self::Error>;
}
