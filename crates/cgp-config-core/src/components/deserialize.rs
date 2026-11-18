use cgp_fork::prelude::*;

use crate::HasConfigValueType;

/// Turns the merged tree into the typed config.
///
/// The preset provider deserializes with serde, coerces env and CLI strings by the target
/// type, and applies an unknown-key policy.
#[cgp_component(ConfigDeserializer)]
pub trait CanDeserializeConfig<Config>: HasConfigValueType + HasErrorType {
    /// Deserializes `value`.
    ///
    /// # Errors
    ///
    /// Type mismatches, missing fields, and (with a denying policy) unknown keys.
    fn deserialize_config(&self, value: Self::ConfigValue) -> Result<Config, Self::Error>;
}
