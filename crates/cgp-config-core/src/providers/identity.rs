use cgp_fork::prelude::*;

use crate::{HasConfigValueType, LayerPreparer, LayerPreparerComponent};

/// Returns every layer unchanged. Wired for sources whose layers already use the current
/// schema and carry no reserved keys: defaults, env, dotenv, CLI, in-memory, dynamic.
pub struct IdentityLayer;

#[cgp_impl(IdentityLayer)]
impl<Source> LayerPreparer<Source>
where
    Self: HasConfigValueType + HasErrorType,
{
    fn prepare_layer(
        &self,
        _source: PhantomData<Source>,
        layer: Self::ConfigValue,
    ) -> Result<Self::ConfigValue, Self::Error> {
        Ok(layer)
    }
}
