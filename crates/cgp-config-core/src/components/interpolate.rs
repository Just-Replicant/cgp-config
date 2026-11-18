use cgp_fork::prelude::*;

use crate::HasConfigValueType;

/// Expands `${...}` references in the strings `Config` opts in with
/// `#[config(interpolate)]`. Every other string stays literal.
///
/// The preset provider is `NoInterpolation`.
#[cgp_component(Interpolator)]
pub trait CanInterpolate<Config>: HasConfigValueType + HasErrorType {
    /// Returns the tree with opted-in strings expanded.
    ///
    /// # Errors
    ///
    /// A missing variable, a cycle, or the interpolation depth limit.
    fn interpolate(&self, tree: Self::ConfigValue) -> Result<Self::ConfigValue, Self::Error>;
}
