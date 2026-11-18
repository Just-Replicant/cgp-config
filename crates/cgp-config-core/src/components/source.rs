use cgp_fork::prelude::*;

use crate::HasConfigValueType;

/// Loads the layer identified by the marker type `Source`.
///
/// `Ok(None)` means an optional source is absent (an optional file that does not exist,
/// an unset variable). A present source that cannot be read or parsed is `Err`.
///
/// Providers implement `SourceLoader<ThatMarker>` for the one marker they serve; a context
/// dispatches per marker with `SourceLoaderComponent: UseDelegate<Table>`.
#[cgp_component(SourceLoader)]
#[derive_delegate(UseDelegate<Source>)]
pub trait CanLoadSource<Source>: HasConfigValueType + HasErrorType {
    /// Loads the layer, or `None` when the optional source is absent.
    ///
    /// # Errors
    ///
    /// When the source is present but cannot be read or parsed, or a required source is
    /// missing.
    fn load_source(
        &self,
        source: PhantomData<Source>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error>;
}
