use core::fmt;

/// A key path made of `&'static str` segments, for the no-alloc tier.
///
/// Typed partial builders and `no_std` targets without an allocator name fields with
/// this type. With `alloc`, `KeyPath` is the owned, dynamic form and
/// `KeyPath::from(StaticKeyPath)` converts.
///
/// ```
/// use cgp_config_core::StaticKeyPath;
///
/// const PORT: StaticKeyPath = StaticKeyPath::new(&["http", "port"]);
/// assert_eq!(PORT.segments(), ["http", "port"]);
/// # #[cfg(feature = "alloc")]
/// assert_eq!(format!("{PORT}"), "http.port");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StaticKeyPath {
    segments: &'static [&'static str],
}

impl StaticKeyPath {
    /// The empty (root) path.
    pub const ROOT: Self = Self { segments: &[] };

    /// A path from its segments.
    #[must_use]
    pub const fn new(segments: &'static [&'static str]) -> Self {
        Self { segments }
    }

    /// The segments, outermost first.
    #[must_use]
    pub const fn segments(&self) -> &'static [&'static str] {
        self.segments
    }

    /// Whether this is the root path.
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.segments.is_empty()
    }
}

impl fmt::Display for StaticKeyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            return f.write_str("(root)");
        }
        for (index, segment) in self.segments.iter().enumerate() {
            if index > 0 {
                f.write_str(".")?;
            }
            f.write_str(segment)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::StaticKeyPath;

    #[test]
    fn root_and_segments() {
        assert!(StaticKeyPath::ROOT.is_root());
        let path = StaticKeyPath::new(&["a", "b"]);
        assert!(!path.is_root());
        assert_eq!(path.segments(), ["a", "b"]);
    }
}
