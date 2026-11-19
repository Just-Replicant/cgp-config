//! Structured log details emitted while loading (design plan §12).
//!
//! Source providers call `CanLog<SourceLoaded>` and `CanLog<LayerSkipped>`; a context wires
//! `LoggerComponent` to `NoopLogger` (the preset default) or to the fork's
//! `TracingLogger` (`cgp_fork::extra::log`, feature `log-tracing`). Details carry origins
//! and counts, never values.

use core::fmt;

use cgp_fork::prelude::*;

use crate::Origin;

/// A source produced a layer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SourceLoaded {
    /// Where the layer came from.
    pub origin: Origin,
    /// Number of leaf values in the layer.
    pub key_count: usize,
}

impl SourceLoaded {
    /// A detail.
    #[must_use]
    pub fn new(origin: Origin, key_count: usize) -> Self {
        Self { origin, key_count }
    }
}

impl fmt::Display for SourceLoaded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "config source loaded: {} ({} keys)",
            self.origin, self.key_count
        )
    }
}

/// An optional source was absent, so its layer was skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LayerSkipped {
    /// The absent source.
    pub origin: Origin,
}

impl LayerSkipped {
    /// A detail.
    #[must_use]
    pub fn new(origin: Origin) -> Self {
        Self { origin }
    }
}

impl fmt::Display for LayerSkipped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "config source skipped (absent): {}", self.origin)
    }
}

/// Drops every detail. The preset logger.
pub struct NoopLogger;

#[cgp_provider(LoggerComponent)]
impl<Context, Detail> Logger<Context, Detail> for NoopLogger {
    fn log(_context: &Context, _detail: Detail) {}
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn details_display_origin_and_count() {
        assert_eq!(
            SourceLoaded::new(Origin::file("app.toml"), 3).to_string(),
            "config source loaded: app.toml (3 keys)"
        );
        assert_eq!(
            LayerSkipped::new(Origin::file("app.local.toml")).to_string(),
            "config source skipped (absent): app.local.toml"
        );
    }
}
