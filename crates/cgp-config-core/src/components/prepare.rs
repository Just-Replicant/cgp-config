use cgp_fork::prelude::*;

use crate::HasConfigValueType;

/// Prepares one loaded layer before it is merged.
///
/// File layers record and strip `$version` and expand `$profiles` (and, from 0.2, run
/// the migration chain first). Env, CLI, defaults, and in-memory layers pass through
/// `IdentityLayer`. Dispatch per marker with `UseDelegate` (the preset's table is
/// `DefaultPreparers`, so a marker without a preparer is a compile error).
///
/// The trait deliberately has no `HasActiveProfile` supertrait: that getter needs `alloc`
/// and an `active_profile: String` field, which `no_std` contexts and contexts without
/// file layers do not have. Only the file preparer (`PrepareFileLayer`) requires
/// `Self: HasActiveProfile` (ADR-001, "Preset shape fixed before P2").
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
