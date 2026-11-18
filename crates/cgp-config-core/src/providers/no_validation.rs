use cgp_fork::prelude::*;

use crate::{ConfigValidator, ConfigValidatorComponent};

/// Accepts every config. The preset provider, and the usual entry for sections without
/// rules in a `UseDelegate` validator table.
pub struct NoValidation;

#[cgp_impl(NoValidation)]
impl<Config> ConfigValidator<Config>
where
    Self: HasErrorType,
{
    fn validate_config(&self, _config: &Config) -> Result<(), Self::Error> {
        Ok(())
    }
}
