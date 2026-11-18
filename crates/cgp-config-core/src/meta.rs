use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt;

use crate::{Origin, Span};

/// One origin a node's value came from or overrode.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Provenance {
    /// The source.
    pub origin: Origin,
    /// The location inside that source, when the parser reports spans.
    pub span: Option<Span>,
}

impl Provenance {
    /// A provenance entry.
    #[must_use]
    pub fn new(origin: Origin, span: Option<Span>) -> Self {
        Self { origin, span }
    }
}

impl fmt::Display for Provenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.span {
            Some(span) => write!(f, "{}:{span}", self.origin),
            None => fmt::Display::fmt(&self.origin, f),
        }
    }
}

/// Metadata carried by every node of the value tree.
///
/// `Meta` never holds configuration values: it records where a value came from, which
/// lower layers it overrode, and flags later stages read. In particular it never holds
/// secret bytes, only the `secret` flag.
///
/// The override chain is newest first: [`shadowed`](Self::shadowed)`[0]` is the value this
/// node replaced, `[1]` the one that value replaced, and so on, capped at
/// `LoadLimits::max_provenance` with [`truncated`](Self::truncated) set when entries were
/// dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Meta {
    /// The source of the winning value.
    pub origin: Origin,
    /// The location of the winning value inside its source.
    pub span: Option<Span>,
    /// Origins this value overrode, newest first.
    pub shadowed: Vec<Provenance>,
    /// Whether older entries were dropped from [`shadowed`](Self::shadowed).
    pub truncated: bool,
    /// The node is (or refers to) a secret. Messages and dumps redact it.
    pub secret: bool,
    /// The interpolator already expanded this node; it is not scanned again.
    pub expanded: bool,
    /// The node came from the overlay of this profile.
    pub profile: Option<Arc<str>>,
    /// The node was rewritten by a migration from this schema version (0.2).
    pub migrated_from: Option<u32>,
}

impl Meta {
    /// Metadata for a value read from `origin`.
    #[must_use]
    pub fn new(origin: Origin) -> Self {
        Self {
            origin,
            ..Self::default()
        }
    }

    /// Sets the span.
    #[must_use]
    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    /// Sets or clears the secret flag.
    #[must_use]
    pub fn with_secret(mut self, secret: bool) -> Self {
        self.secret = secret;
        self
    }

    /// Sets the profile name.
    #[must_use]
    pub fn with_profile(mut self, profile: impl Into<Arc<str>>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// The winning origin and span as one [`Provenance`].
    #[must_use]
    pub fn provenance(&self) -> Provenance {
        Provenance::new(self.origin.clone(), self.span)
    }

    /// Records that this node's value replaced `older`, keeping at most `cap` entries.
    ///
    /// The new chain is this node's own chain, then `older`'s winning origin, then
    /// `older`'s chain. That order makes provenance independent of how layers are
    /// grouped: `(a ⊕ b) ⊕ c` and `a ⊕ (b ⊕ c)` produce the same chain.
    pub fn shadow(&mut self, older: &Self, cap: usize) {
        let mut truncated = self.truncated || older.truncated;
        let wanted = self.shadowed.len() + 1 + older.shadowed.len();
        if self.shadowed.len() < cap {
            self.shadowed.push(older.provenance());
        }
        for entry in &older.shadowed {
            if self.shadowed.len() >= cap {
                break;
            }
            self.shadowed.push(entry.clone());
        }
        if self.shadowed.len() > cap {
            self.shadowed.truncate(cap);
        }
        if wanted > self.shadowed.len() {
            truncated = true;
        }
        self.truncated = truncated;
    }

    /// A display adapter: `origin[:line:col]`.
    #[must_use]
    pub fn location(&self) -> impl fmt::Display + '_ {
        Location(self)
    }
}

struct Location<'a>(&'a Meta);

impl fmt::Display for Location<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.span {
            Some(span) => write!(f, "{}:{span}", self.0.origin),
            None => fmt::Display::fmt(&self.0.origin, f),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;

    use super::{Meta, Provenance};
    use crate::{Origin, Span};

    fn at(name: &str) -> Meta {
        Meta::new(Origin::memory(name))
    }

    #[test]
    fn shadow_builds_a_newest_first_chain() {
        let a = at("a");
        let mut b = at("b");
        b.shadow(&a, 16);
        let mut c = at("c");
        c.shadow(&b, 16);
        let names: vec::Vec<_> = c.shadowed.iter().map(|p| p.origin.to_string()).collect();
        assert_eq!(names, ["memory b", "memory a"]);
        assert!(!c.truncated);
    }

    #[test]
    fn shadow_respects_the_cap() {
        let mut node = at("x");
        let mut older = at("o");
        older.shadowed = vec![Provenance::new(Origin::Defaults, None); 3];
        node.shadow(&older, 2);
        assert_eq!(node.shadowed.len(), 2);
        assert!(node.truncated);

        let mut zero = at("z");
        zero.shadow(&at("y"), 0);
        assert!(zero.shadowed.is_empty());
        assert!(zero.truncated);

        let mut over = at("w");
        over.shadowed = vec![Provenance::default(); 4];
        over.shadow(&at("v"), 2);
        assert_eq!(over.shadowed.len(), 2);
        assert!(over.truncated);
    }

    #[test]
    fn builders_and_location() {
        let meta = Meta::new(Origin::file("c.toml"))
            .with_span(Span::new(0, 1, 3, 4))
            .with_secret(true)
            .with_profile("prod");
        assert!(meta.secret);
        assert_eq!(meta.profile.as_deref(), Some("prod"));
        assert_eq!(meta.location().to_string(), "c.toml:3:4");
        assert_eq!(meta.provenance().to_string(), "c.toml:3:4");
        assert_eq!(
            Meta::new(Origin::Defaults).location().to_string(),
            "defaults"
        );
        assert_eq!(
            Provenance::new(Origin::Defaults, None).to_string(),
            "defaults"
        );
    }
}
