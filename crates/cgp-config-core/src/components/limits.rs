use cgp_fork::prelude::*;

use crate::LoadLimits;

/// The [`LoadLimits`] of a context.
///
/// A component rather than an auto-getter, so a preset can supply the defaults
/// (`LoadLimitsComponent: DefaultLoadLimits`) and a context can read its own field
/// instead (`LoadLimitsComponent: LoadLimitsField`).
#[cgp_component {
    name: LoadLimitsComponent,
    provider: LoadLimitsGetter,
}]
pub trait HasLoadLimits {
    /// The limits to apply.
    fn load_limits(&self) -> LoadLimits;
}
