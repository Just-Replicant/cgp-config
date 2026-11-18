use cgp_fork::prelude::*;

use crate::{HasConfigValueType, Interpolator, InterpolatorComponent};

/// Returns the tree unchanged: every string stays literal. The preset provider.
pub struct NoInterpolation;

#[cgp_impl(NoInterpolation)]
impl<Config> Interpolator<Config>
where
    Self: HasConfigValueType + HasErrorType,
{
    fn interpolate(&self, tree: Self::ConfigValue) -> Result<Self::ConfigValue, Self::Error> {
        Ok(tree)
    }
}
