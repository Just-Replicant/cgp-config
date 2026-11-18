use cgp_fork::prelude::*;

use crate::HasConfigValueType;

/// Folds a type-level list of source markers (`Product![A, B, C]`) into one tree.
///
/// Precedence is list order: later sources win. `CollectInOrder` loads, prepares, and
/// merges each marker in turn.
#[cgp_component(LayerCollector)]
pub trait CanCollectLayers<Sources>: HasConfigValueType + HasErrorType {
    /// Merges every source of `Sources` onto `acc`.
    ///
    /// # Errors
    ///
    /// The first load, preparation, or merge error.
    fn collect_layers(
        &self,
        acc: Self::ConfigValue,
        sources: PhantomData<Sources>,
    ) -> Result<Self::ConfigValue, Self::Error>;
}
