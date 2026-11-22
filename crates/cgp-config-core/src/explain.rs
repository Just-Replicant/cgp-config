//! [`LoadStamp`]: the redacted picture of one load, for `explain`, `dump`, and the
//! fingerprint.
//!
//! The stamp is built from the value tree after secrets are marked and before they are
//! resolved, so it can hold origins and spans but never secret bytes. A secret node is
//! the text `[REDACTED]`. A reference table may also record the *redacted* reference
//! (`env:DB_PASSWORD`, `file:<redacted #…>`), which is not the secret.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::error::fnv1a64;
use crate::{KeyPath, Origin, Provenance, Span};

/// One node of a load, safe to print.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExplainNode {
    /// Where the node sits. The root path is empty.
    pub path: KeyPath,
    /// The value as text. Secret nodes are `[REDACTED]`.
    pub value: String,
    /// The winning source.
    pub origin: Origin,
    /// The winning span, when the source has one.
    pub span: Option<Span>,
    /// Sources this value overrode, newest first.
    pub shadowed: Vec<Provenance>,
    /// The node is a secret. [`value`](Self::value) does not contain it.
    pub secret: bool,
    /// The redacted reference (`env:NAME`, `file:name`, …), when the node is a
    /// reference table. Never the secret and never an unsafe path.
    pub reference: Option<String>,
}

impl ExplainNode {
    /// A node. `value` must already be safe to print.
    #[must_use]
    pub fn new(
        path: KeyPath,
        value: String,
        origin: Origin,
        span: Option<Span>,
        shadowed: Vec<Provenance>,
        secret: bool,
        reference: Option<String>,
    ) -> Self {
        Self {
            path,
            value,
            origin,
            span,
            shadowed,
            secret,
            reference,
        }
    }
}

/// The redacted record of one load.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoadStamp {
    /// FNV-1a 64 of [`dump_json`](Self::dump_json). Stable across Rust releases.
    pub fingerprint: u64,
    /// The winning origin of the root node.
    pub origin: Origin,
    /// Every non-secret node, plus secret nodes themselves (not their children).
    pub nodes: Vec<ExplainNode>,
    /// Canonical JSON: map keys sorted, secrets replaced with `"[REDACTED]"`.
    pub dump_json: String,
}

impl LoadStamp {
    /// A stamp. The fingerprint is the FNV-1a 64 of `dump_json`.
    #[must_use]
    pub fn new(origin: Origin, nodes: Vec<ExplainNode>, dump_json: String) -> Self {
        let fingerprint = fnv1a64(dump_json.as_bytes());
        Self {
            fingerprint,
            origin,
            nodes,
            dump_json,
        }
    }

    /// The fingerprint as 16 lowercase hex digits.
    #[must_use]
    pub fn fingerprint_hex(&self) -> String {
        alloc::format!("{:016x}", self.fingerprint)
    }

    /// The node at `path` (`http.port`, `servers[0].host`). An empty path and `(root)`
    /// name the root.
    #[must_use]
    pub fn explain(&self, path: &str) -> Option<Explanation<'_>> {
        let wanted = if path.is_empty() || path == "(root)" {
            KeyPath::root()
        } else {
            KeyPath::parse(path).ok()?
        };
        self.nodes
            .iter()
            .find(|node| node.path == wanted)
            .map(Explanation)
    }
}

impl fmt::Debug for LoadStamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadStamp")
            .field("fingerprint", &self.fingerprint_hex())
            .field("origin", &self.origin)
            .field("nodes", &self.nodes.len())
            .finish()
    }
}

/// A borrowed explanation of one path. `Display` is the text `explain` prints.
#[derive(Clone, Copy)]
pub struct Explanation<'a>(&'a ExplainNode);

impl Explanation<'_> {
    /// The node.
    #[must_use]
    pub fn node(&self) -> &ExplainNode {
        self.0
    }
}

impl fmt::Display for Explanation<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let node = self.0;
        if node.path.is_empty() {
            write!(f, "(root): {}", node.value)?;
        } else {
            write!(f, "{}: {}", node.path, node.value)?;
        }
        if let Some(reference) = &node.reference {
            write!(f, "\n  reference: {reference}")?;
        }
        write!(f, "\n  origin: {}", node.origin)?;
        if let Some(span) = node.span {
            write!(f, ":{span}")?;
        }
        if !node.shadowed.is_empty() {
            write!(f, "\n  overrides:")?;
            for prior in &node.shadowed {
                write!(f, " {prior}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::{String, ToString};
    use alloc::vec;

    use super::{ExplainNode, LoadStamp};
    use crate::{KeyPath, Origin, Provenance};

    #[test]
    fn explain_and_fingerprint_do_not_depend_on_hasher_seeds() {
        let secret = ExplainNode::new(
            KeyPath::from("password"),
            String::from("[REDACTED]"),
            Origin::env("APP__PASSWORD"),
            None,
            vec![Provenance::new(Origin::file("app.toml"), None)],
            true,
            Some(String::from("env:DB_PASSWORD")),
        );
        let stamp = LoadStamp::new(
            Origin::file("app.toml"),
            vec![secret],
            String::from(r#"{"password":"[REDACTED]"}"#),
        );
        assert_eq!(stamp.fingerprint_hex().len(), 16);
        let shown = stamp.explain("password").unwrap().to_string();
        assert!(shown.contains("[REDACTED]"), "{shown}");
        assert!(shown.contains("env:DB_PASSWORD"), "{shown}");
        assert!(shown.contains("overrides: app.toml"), "{shown}");
        assert!(stamp.explain("missing").is_none());
        assert!(stamp.explain("(root)").is_none());
        let again = LoadStamp::new(
            Origin::Defaults,
            vec![],
            String::from(r#"{"password":"[REDACTED]"}"#),
        );
        assert_eq!(stamp.fingerprint, again.fingerprint);
    }
}
