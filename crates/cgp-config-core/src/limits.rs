/// Hard limits applied while reading, parsing, merging, and interpolating.
///
/// Every limit is checked by the stage that can exceed it and reported as
/// `ConfigError::LimitExceeded` naming the limit. The defaults are generous for real
/// configuration files and small enough that a hostile document cannot exhaust memory
/// or the stack.
///
/// | Field | Default |
/// |---|---|
/// | [`max_bytes`](Self::max_bytes) | 10 MiB |
/// | [`max_depth`](Self::max_depth) | 64 |
/// | [`max_keys`](Self::max_keys) | 100 000 |
/// | [`max_interpolation_depth`](Self::max_interpolation_depth) | 8 |
/// | [`max_provenance`](Self::max_provenance) | 16 |
///
/// ```
/// use cgp_config_core::LoadLimits;
///
/// let limits = LoadLimits::DEFAULT.with_max_depth(16).with_max_bytes(64 * 1024);
/// assert_eq!(limits.max_depth, 16);
/// assert_eq!(limits.max_keys, LoadLimits::DEFAULT.max_keys);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct LoadLimits {
    /// Largest accepted source text, in bytes.
    pub max_bytes: usize,
    /// Deepest accepted nesting of maps and arrays. The root is depth 0.
    pub max_depth: usize,
    /// Most map entries plus array elements in one document or merged tree.
    pub max_keys: usize,
    /// Deepest accepted chain of interpolation references.
    pub max_interpolation_depth: usize,
    /// Most shadowed origins kept per node. Further origins set a truncation flag.
    pub max_provenance: usize,
}

impl LoadLimits {
    /// The documented defaults.
    pub const DEFAULT: Self = Self {
        max_bytes: 10 * 1024 * 1024,
        max_depth: 64,
        max_keys: 100_000,
        max_interpolation_depth: 8,
        max_provenance: 16,
    };

    /// No limits at all. Intended for trusted, generated input in tests and tools.
    pub const UNLIMITED: Self = Self {
        max_bytes: usize::MAX,
        max_depth: usize::MAX,
        max_keys: usize::MAX,
        max_interpolation_depth: usize::MAX,
        max_provenance: usize::MAX,
    };

    /// Replaces [`max_bytes`](Self::max_bytes).
    #[must_use]
    pub const fn with_max_bytes(mut self, value: usize) -> Self {
        self.max_bytes = value;
        self
    }

    /// Replaces [`max_depth`](Self::max_depth).
    #[must_use]
    pub const fn with_max_depth(mut self, value: usize) -> Self {
        self.max_depth = value;
        self
    }

    /// Replaces [`max_keys`](Self::max_keys).
    #[must_use]
    pub const fn with_max_keys(mut self, value: usize) -> Self {
        self.max_keys = value;
        self
    }

    /// Replaces [`max_interpolation_depth`](Self::max_interpolation_depth).
    #[must_use]
    pub const fn with_max_interpolation_depth(mut self, value: usize) -> Self {
        self.max_interpolation_depth = value;
        self
    }

    /// Replaces [`max_provenance`](Self::max_provenance).
    #[must_use]
    pub const fn with_max_provenance(mut self, value: usize) -> Self {
        self.max_provenance = value;
        self
    }
}

impl Default for LoadLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::LoadLimits;

    #[test]
    fn defaults_match_the_documented_values() {
        let limits = LoadLimits::default();
        assert_eq!(limits.max_bytes, 10 * 1024 * 1024);
        assert_eq!(limits.max_depth, 64);
        assert_eq!(limits.max_keys, 100_000);
        assert_eq!(limits.max_interpolation_depth, 8);
        assert_eq!(limits.max_provenance, 16);
    }

    #[test]
    fn builders_replace_one_field() {
        let limits = LoadLimits::DEFAULT
            .with_max_bytes(1)
            .with_max_depth(2)
            .with_max_keys(3)
            .with_max_interpolation_depth(4)
            .with_max_provenance(5);
        assert_eq!(
            (
                limits.max_bytes,
                limits.max_depth,
                limits.max_keys,
                limits.max_interpolation_depth,
                limits.max_provenance
            ),
            (1, 2, 3, 4, 5)
        );
        assert_eq!(LoadLimits::UNLIMITED.max_depth, usize::MAX);
    }
}
