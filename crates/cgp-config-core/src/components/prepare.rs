use cgp_fork::prelude::*;

use crate::HasConfigValueType;

/// Prepares one loaded layer before it is merged.
///
/// File layers record and strip `$version` and expand `$profiles` (and, from 0.2, run
/// the migration chain first). Env, CLI, defaults, and in-memory layers pass through
/// `IdentityLayer`. Dispatch per marker with `UseDelegate`.
#[cgp_component(LayerPreparer)]
#[derive_delegate(UseDelegate<Source>)]
pub trait CanPrepareLayer<Source>: HasConfigValueType + HasErrorType {
    /// Returns the prepared layer.
    ///
    /// # Errors
    ///
    /// When preparation fails (a malformed `$version`, a missing required profile).
    fn prepare_layer(
        &self,
        source: PhantomData<Source>,
        layer: Self::ConfigValue,
    ) -> Result<Self::ConfigValue, Self::Error>;
}
