use cgp_fork::prelude::*;

/// The intermediate tree every source produces and every later stage consumes.
///
/// The default backend is `cgp_config_value::ConfigValue`. `Clone` must be cheap (the
/// default tree is reference-counted). There is deliberately no `Debug` bound: debugging
/// goes through the backend's redacting `Debug`, which prints `[REDACTED]` for secret nodes.
///
/// Wire it with `ConfigValueTypeProviderComponent: UseType<ConfigValue>` or, in a preset or
/// table, `type ConfigValue = ConfigValue`.
#[cgp_type]
pub trait HasConfigValueType {
    /// The tree type.
    type ConfigValue: Clone;
}

/// The empty document the layer fold starts from.
///
/// Provided next to the value type (`EmptyMap` for the default tree), not as a free
/// function on the context.
#[cgp_component(EmptyConfigValue)]
pub trait HasEmptyConfigValue: HasConfigValueType {
    /// A new empty document.
    fn empty_config_value(&self) -> Self::ConfigValue;
}

/// The value tree type of `Context`.
pub type ConfigValueOf<Context> = <Context as HasConfigValueType>::ConfigValue;
