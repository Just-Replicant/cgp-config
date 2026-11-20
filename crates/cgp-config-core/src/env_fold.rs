//! How env names fold to document keys (design plan §6.3): ASCII lowercase, `-` as `_`,
//! and an `_` at each word boundary of a mixed-case name, so `MaxConnections`,
//! `MAX_CONNECTIONS`, and `max-connections` all read `max_connections`.
//!
//! A word starts at an uppercase letter that follows a lowercase letter or a digit, or
//! that ends a run of capitals before a lowercase letter (`HTTPServer` →
//! `http_server`). An all-uppercase or all-lowercase segment only changes case.
//!
//! The rule is shared by the env layer, the `.env` layer, and the compile-time key check
//! ([`ConfigMeta::checked`](crate::ConfigMeta::checked)), so a name the check accepts
//! cannot become ambiguous at run time.

/// Whether an `_` goes before `bytes[index]` when folding.
#[must_use]
pub const fn starts_word(bytes: &[u8], index: usize) -> bool {
    if index == 0 || index >= bytes.len() || !bytes[index].is_ascii_uppercase() {
        return false;
    }
    let previous = bytes[index - 1];
    if previous.is_ascii_lowercase() || previous.is_ascii_digit() {
        return true;
    }
    previous.is_ascii_uppercase()
        && index + 1 < bytes.len()
        && bytes[index + 1].is_ascii_lowercase()
}

/// The folded form of one env segment (or of a document key, to compare with one).
///
/// ```
/// use cgp_config_core::env_fold::fold;
///
/// assert_eq!(fold("MAX_CONNECTIONS"), "max_connections");
/// assert_eq!(fold("MaxConnections"), "max_connections");
/// assert_eq!(fold("maxConnections"), "max_connections");
/// assert_eq!(fold("HTTPServer"), "http_server");
/// assert_eq!(fold("tls-cert"), "tls_cert");
/// ```
#[cfg(feature = "alloc")]
#[must_use]
pub fn fold(segment: &str) -> alloc::string::String {
    let bytes = segment.as_bytes();
    let mut out = alloc::string::String::with_capacity(segment.len() + 4);
    for (index, c) in segment.char_indices() {
        if starts_word(bytes, index) {
            out.push('_');
        }
        match c {
            '-' => out.push('_'),
            c => out.push(c.to_ascii_lowercase()),
        }
    }
    out
}

/// Whether env segment `segment` selects the document key `key`: equal ignoring ASCII
/// case and `-`/`_`, or equal once both are [folded](fold).
///
/// ```
/// use cgp_config_core::env_fold::matches;
///
/// assert!(matches("max_connections", "MAX_CONNECTIONS"));
/// assert!(matches("max_connections", "MaxConnections"));
/// assert!(matches("maxConnections", "MAXCONNECTIONS"));
/// assert!(matches("tls-cert", "TLS_CERT"));
/// assert!(!matches("max_connections", "MAXCONNECTIONS"));
/// ```
#[cfg(feature = "alloc")]
#[must_use]
pub fn matches(key: &str, segment: &str) -> bool {
    let loose = |a: &str, b: &str| {
        a.len() == b.len()
            && a.bytes().zip(b.bytes()).all(|(x, y)| {
                let norm = |c: u8| {
                    if c == b'-' {
                        b'_'
                    } else {
                        c.to_ascii_lowercase()
                    }
                };
                norm(x) == norm(y)
            })
    };
    loose(key, segment) || fold(key) == fold(segment)
}
