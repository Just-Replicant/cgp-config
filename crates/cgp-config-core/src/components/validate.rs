use cgp_fork::prelude::*;

/// Checks a resolved config. Runs after secret resolution and before publication.
///
/// Dispatch per config type with `ConfigValidatorComponent: UseDelegate<Table>`, so each
/// section can have its own validator.
#[cgp_component(ConfigValidator)]
#[derive_delegate(UseDelegate<Config>)]
pub trait CanValidateConfig<Config>: HasErrorType {
    /// Validates `config`.
    ///
    /// # Errors
    ///
    /// Every failure found, aggregated into one error.
    fn validate_config(&self, config: &Config) -> Result<(), Self::Error>;
}
