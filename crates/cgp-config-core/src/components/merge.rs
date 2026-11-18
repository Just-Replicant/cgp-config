use cgp_fork::prelude::*;

use crate::HasConfigValueType;

/// Merges a later layer (`overlay`) onto the accumulated tree (`base`).
///
/// Merging is fallible: a shape conflict (a scalar over a map, an array over a map, ...)
/// is `ConfigError::MergeConflict` with both origins, never a silent replacement.
#[cgp_component(LayerMerger)]
pub trait CanMergeLayers: HasConfigValueType + HasErrorType {
    /// Returns `base` with `overlay` merged on top.
    ///
    /// # Errors
    ///
    /// On a shape conflict or when the merged tree exceeds a load limit.
    fn merge_layers(
        &self,
        base: Self::ConfigValue,
        overlay: Self::ConfigValue,
    ) -> Result<Self::ConfigValue, Self::Error>;
}
