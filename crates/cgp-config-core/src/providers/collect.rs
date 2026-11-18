use cgp_fork::prelude::*;

use crate::{
    CanCollectLayers, CanLoadSource, CanMergeLayers, CanPrepareLayer, HasConfigValueType,
    LayerCollector, LayerCollectorComponent,
};

/// Folds `Product![A, B, ..]` in list order: for each marker, load the layer, prepare it,
/// and merge it onto the accumulator. An absent optional source leaves the accumulator
/// unchanged. Later markers win.
///
/// ```text
/// Product![Defaults, ConfigFile, ProfileFile, EnvVars]
///          lowest  ──────────────────────▶  highest
/// ```
pub struct CollectInOrder;

#[cgp_impl(CollectInOrder)]
impl<Source, Rest> LayerCollector<Cons<Source, Rest>>
where
    Self: CanLoadSource<Source> + CanPrepareLayer<Source> + CanMergeLayers + CanCollectLayers<Rest>,
{
    fn collect_layers(
        &self,
        acc: Self::ConfigValue,
        _sources: PhantomData<Cons<Source, Rest>>,
    ) -> Result<Self::ConfigValue, Self::Error> {
        let acc = match self.load_source(PhantomData::<Source>)? {
            Some(layer) => {
                let layer = self.prepare_layer(PhantomData::<Source>, layer)?;
                self.merge_layers(acc, layer)?
            }
            None => acc,
        };
        self.collect_layers(acc, PhantomData::<Rest>)
    }
}

#[cgp_impl(CollectInOrder)]
impl LayerCollector<Nil>
where
    Self: HasConfigValueType + HasErrorType,
{
    fn collect_layers(
        &self,
        acc: Self::ConfigValue,
        _sources: PhantomData<Nil>,
    ) -> Result<Self::ConfigValue, Self::Error> {
        Ok(acc)
    }
}
