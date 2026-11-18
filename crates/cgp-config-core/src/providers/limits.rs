use cgp_fork::prelude::*;

use crate::{LoadLimits, LoadLimitsComponent, LoadLimitsGetter};

/// [`LoadLimits::DEFAULT`], as a constant getter. The preset provider.
pub struct DefaultLoadLimits;

#[cgp_impl(DefaultLoadLimits: LoadLimitsComponent)]
impl LoadLimitsGetter {
    fn load_limits(&self) -> LoadLimits {
        LoadLimits::DEFAULT
    }
}

/// Reads the context's `load_limits: LoadLimits` field.
pub struct LoadLimitsField;

#[cgp_impl(LoadLimitsField: LoadLimitsComponent)]
impl LoadLimitsGetter
where
    Self: HasField<Symbol!("load_limits"), Value = LoadLimits>,
{
    fn load_limits(&self) -> LoadLimits {
        *self.get_field(PhantomData::<Symbol!("load_limits")>)
    }
}
