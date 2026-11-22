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

/// A load finished. The fingerprint is the FNV-1a 64 of the canonical redacted JSON
/// ([`LoadReport::fingerprint`](crate::LoadReport::fingerprint)); `origin` is the winning
/// origin of the root. The detail carries no values.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConfigLoaded {
    /// The winning origin of the root node.
    pub origin: Origin,
    /// [`LoadReport::fingerprint`](crate::LoadReport::fingerprint), as a number.
    pub fingerprint: u64,
}

impl ConfigLoaded {
    /// A detail.
    #[must_use]
    pub fn new(origin: Origin, fingerprint: u64) -> Self {
        Self {
            origin,
            fingerprint,
        }
    }
}

impl fmt::Display for ConfigLoaded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "config loaded from {} fingerprint {:016x}",
            self.origin, self.fingerprint
        )
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
        let loaded = ConfigLoaded::new(Origin::file("app.toml"), 0xabc);
        let text = loaded.to_string();
        assert!(text.contains("app.toml"), "{text}");
        assert!(text.contains("0000000000000abc"), "{text}");
        assert!(!text.contains("password"));
    }
}
