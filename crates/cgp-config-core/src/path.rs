use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::{self, Write as _};
use core::str::FromStr;

use crate::StaticKeyPath;

/// One step of a [`KeyPath`]: a map key or an array index.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PathSegment {
    /// A map key. File keys keep their case.
    Key(Arc<str>),
    /// An array index.
    Index(usize),
}

impl PathSegment {
    /// The key, when this segment is a key.
    #[must_use]
    pub fn as_key(&self) -> Option<&str> {
        match self {
            Self::Key(key) => Some(key),
            Self::Index(_) => None,
        }
    }

    /// The index, when this segment is an index.
    #[must_use]
    pub fn as_index(&self) -> Option<usize> {
        match self {
            Self::Key(_) => None,
            Self::Index(index) => Some(*index),
        }
    }
}

impl From<&str> for PathSegment {
    fn from(key: &str) -> Self {
        Self::Key(Arc::from(key))
    }
}

impl From<Arc<str>> for PathSegment {
    fn from(key: Arc<str>) -> Self {
        Self::Key(key)
    }
}

impl From<usize> for PathSegment {
    fn from(index: usize) -> Self {
        Self::Index(index)
    }
}

/// The location of a node inside a configuration tree, such as `http.port` or
/// `servers[0].host`.
///
/// `Display` writes keys separated by `.` and indices as `[n]`. A key that is empty,
/// contains `.`, `[`, `]`, `"`, `\`, or whitespace, or is exactly the text `(root)` is
/// written in double quotes with `\"` and `\\` escapes, so [`KeyPath::parse`] reads back
/// every path `Display` writes. The root path displays as `(root)` (unquoted); a key named
/// `(root)` displays as `"(root)"`.
///
/// ```
/// use cgp_config_core::KeyPath;
///
/// let path = KeyPath::parse("servers[0].host")?;
/// assert_eq!(path.len(), 3);
/// assert_eq!(path.to_string(), "servers[0].host");
///
/// let odd = KeyPath::root().join("routes").join("api.v1");
/// assert_eq!(odd.to_string(), r#"routes."api.v1""#);
/// assert_eq!(KeyPath::parse(&odd.to_string())?, odd);
/// # Ok::<(), cgp_config_core::KeyPathParseError>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyPath {
    segments: Vec<PathSegment>,
}

impl KeyPath {
    /// The empty path, naming the document root.
    #[must_use]
    pub const fn root() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// A path from segments, outermost first.
    #[must_use]
    pub fn from_segments(segments: impl IntoIterator<Item = PathSegment>) -> Self {
        Self {
            segments: segments.into_iter().collect(),
        }
    }

    /// Parses the syntax `Display` writes.
    ///
    /// # Errors
    ///
    /// Returns [`KeyPathParseError`] for an unterminated quote, a malformed or overflowing
    /// index, an empty unquoted segment (`a..b`, a leading or trailing `.`), or a stray `]`.
    pub fn parse(text: &str) -> Result<Self, KeyPathParseError> {
        parse_path(text)
    }

    /// Parses `text`, or treats the whole text as one key when it is not valid path syntax.
    ///
    /// Used by `From<&str>` so constructors such as `ConfigError::validation("http.port", ..)`
    /// never fail.
    #[must_use]
    pub fn parse_lenient(text: &str) -> Self {
        Self::parse(text).unwrap_or_else(|_| Self::root().join(text))
    }

    /// The segments, outermost first.
    #[must_use]
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    /// Number of segments.
    #[must_use]
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Whether this is the root path.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Whether this is the root path. Same as [`is_empty`](Self::is_empty).
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    /// Appends one segment in place.
    pub fn push(&mut self, segment: impl Into<PathSegment>) {
        self.segments.push(segment.into());
    }

    /// Removes and returns the last segment.
    pub fn pop(&mut self) -> Option<PathSegment> {
        self.segments.pop()
    }

    /// A copy of this path with one more segment.
    #[must_use]
    pub fn join(&self, segment: impl Into<PathSegment>) -> Self {
        let mut path = self.clone();
        path.push(segment);
        path
    }

    /// A copy of this path followed by every segment of `other`.
    #[must_use]
    pub fn concat(&self, other: &Self) -> Self {
        let mut path = self.clone();
        path.segments.extend(other.segments.iter().cloned());
        path
    }

    /// The path without its last segment, or `None` at the root.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        let (_, rest) = self.segments.split_last()?;
        Some(Self {
            segments: rest.to_vec(),
        })
    }

    /// The last segment, or `None` at the root.
    #[must_use]
    pub fn last(&self) -> Option<&PathSegment> {
        self.segments.last()
    }

    /// Whether `self` is `other` or one of its ancestors.
    #[must_use]
    pub fn is_prefix_of(&self, other: &Self) -> bool {
        other.segments.starts_with(&self.segments)
    }

    /// Iterates over the segments.
    pub fn iter(&self) -> core::slice::Iter<'_, PathSegment> {
        self.segments.iter()
    }
}

impl<'a> IntoIterator for &'a KeyPath {
    type Item = &'a PathSegment;
    type IntoIter = core::slice::Iter<'a, PathSegment>;

    fn into_iter(self) -> Self::IntoIter {
        self.segments.iter()
    }
}

impl FromIterator<PathSegment> for KeyPath {
    fn from_iter<I: IntoIterator<Item = PathSegment>>(iter: I) -> Self {
        Self::from_segments(iter)
    }
}

impl From<&str> for KeyPath {
    fn from(text: &str) -> Self {
        Self::parse_lenient(text)
    }
}

impl From<StaticKeyPath> for KeyPath {
    fn from(path: StaticKeyPath) -> Self {
        path.segments()
            .iter()
            .map(|s| PathSegment::from(*s))
            .collect()
    }
}

impl FromStr for KeyPath {
    type Err = KeyPathParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

/// How the root path displays. A key with exactly this text is quoted, so the two never
/// print the same.
const ROOT_TEXT: &str = "(root)";

fn needs_quotes(key: &str) -> bool {
    key.is_empty()
        || key == ROOT_TEXT
        || key
            .chars()
            .any(|c| matches!(c, '.' | '[' | ']' | '"' | '\\') || c.is_whitespace())
}

fn write_key(f: &mut fmt::Formatter<'_>, key: &str) -> fmt::Result {
    if !needs_quotes(key) {
        return f.write_str(key);
    }
    f.write_char('"')?;
    for c in key.chars() {
        if matches!(c, '"' | '\\') {
            f.write_char('\\')?;
        }
        f.write_char(c)?;
    }
    f.write_char('"')
}

impl fmt::Display for KeyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            return f.write_str(ROOT_TEXT);
        }
        for (position, segment) in self.segments.iter().enumerate() {
            match segment {
                PathSegment::Key(key) => {
                    if position > 0 {
                        f.write_char('.')?;
                    }
                    write_key(f, key)?;
                }
                PathSegment::Index(index) => write!(f, "[{index}]")?,
            }
        }
        Ok(())
    }
}

/// Why [`KeyPath::parse`] rejected its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyPathParseError {
    /// Byte offset of the problem in the input.
    pub offset: usize,
    /// What was wrong.
    pub reason: &'static str,
}

impl fmt::Display for KeyPathParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid key path at byte {}: {}",
            self.offset, self.reason
        )
    }
}

impl core::error::Error for KeyPathParseError {}

fn parse_path(text: &str) -> Result<KeyPath, KeyPathParseError> {
    let err = |offset, reason| KeyPathParseError { offset, reason };
    let bytes = text.as_bytes();
    let mut segments = Vec::new();
    // Only the whole text `(root)` is the root. Display quotes a key with that text, so an
    // unquoted `(root)` never stands for a key.
    if text == ROOT_TEXT || text.is_empty() {
        return Ok(KeyPath::root());
    }
    let mut i = 0;
    // `expect_key` is true at the start and right after a `.`.
    let mut expect_key = true;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => {
                if expect_key && !segments.is_empty() {
                    return Err(err(i, "expected a key after `.`"));
                }
                let close = text[i..]
                    .find(']')
                    .map(|p| p + i)
                    .ok_or_else(|| err(i, "unterminated `[`"))?;
                let digits = &text[i + 1..close];
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(err(i + 1, "an index must be decimal digits"));
                }
                let index = digits
                    .parse::<usize>()
                    .map_err(|_| err(i + 1, "index overflows usize"))?;
                segments.push(PathSegment::Index(index));
                i = close + 1;
                expect_key = false;
            }
            b'.' => {
                if expect_key {
                    return Err(err(i, "empty key segment"));
                }
                expect_key = true;
                i += 1;
            }
            b']' => return Err(err(i, "unmatched `]`")),
            b'"' => {
                if !expect_key {
                    return Err(err(i, "expected `.` or `[` before a key"));
                }
                let mut key = alloc::string::String::new();
                let mut j = i + 1;
                let mut closed = false;
                let mut chars = text[j..].char_indices();
                while let Some((offset, c)) = chars.next() {
                    match c {
                        '\\' => {
                            let (_, escaped) = chars
                                .next()
                                .ok_or_else(|| err(j + offset, "dangling `\\`"))?;
                            key.push(escaped);
                        }
                        '"' => {
                            j += offset + 1;
                            closed = true;
                            break;
                        }
                        other => key.push(other),
                    }
                }
                if !closed {
                    return Err(err(i, "unterminated quoted key"));
                }
                segments.push(PathSegment::Key(Arc::from(key)));
                i = j;
                expect_key = false;
            }
            _ => {
                if !expect_key {
                    return Err(err(i, "expected `.` or `[` before a key"));
                }
                let end = text[i..]
                    .find(['.', '[', ']', '"'])
                    .map_or(text.len(), |p| p + i);
                segments.push(PathSegment::Key(Arc::from(&text[i..end])));
                i = end;
                expect_key = false;
            }
        }
    }
    if expect_key {
        return Err(err(text.len(), "trailing `.`"));
    }
    Ok(KeyPath { segments })
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;

    use super::{KeyPath, PathSegment};

    #[test]
    fn display_and_parse() {
        let path = KeyPath::root().join("a").join(2usize).join("b");
        assert_eq!(path.to_string(), "a[2].b");
        assert_eq!(KeyPath::parse("a[2].b").unwrap(), path);
        assert_eq!(KeyPath::root().to_string(), "(root)");
        assert_eq!(KeyPath::parse("(root)").unwrap(), KeyPath::root());
        assert_eq!(KeyPath::parse("").unwrap(), KeyPath::root());
        assert_eq!(KeyPath::parse("[0][1]").unwrap().len(), 2);
    }

    #[test]
    fn quoting_round_trips() {
        let path = KeyPath::root()
            .join("")
            .join("a.b")
            .join("q\"uote")
            .join("back\\slash")
            .join("sp ace");
        let text = path.to_string();
        assert_eq!(text, r#"""."a.b"."q\"uote"."back\\slash"."sp ace""#);
        assert_eq!(KeyPath::parse(&text).unwrap(), path);
    }

    #[test]
    fn a_key_named_root_round_trips() {
        let key = KeyPath::root().join("(root)");
        assert_eq!(key.to_string(), r#""(root)""#);
        assert_eq!(KeyPath::parse(&key.to_string()).unwrap(), key);
        assert_ne!(key.to_string(), KeyPath::root().to_string());

        let nested = KeyPath::root().join("a").join("(root)").join(0usize);
        assert_eq!(nested.to_string(), r#"a."(root)"[0]"#);
        assert_eq!(KeyPath::parse(&nested.to_string()).unwrap(), nested);

        // Similar texts are ordinary keys and stay unquoted.
        let similar = KeyPath::root().join("(root").join("root)");
        assert_eq!(similar.to_string(), "(root.root)");
        assert_eq!(KeyPath::parse(&similar.to_string()).unwrap(), similar);
    }

    #[test]
    fn rejects_malformed_paths() {
        for bad in [
            "a..b",
            ".a",
            "a.",
            "a[",
            "a[x]",
            "a[]",
            "a]",
            "\"open",
            "a\"b\"",
            "a.[0]",
            "a[0]b",
            "\"x\\",
            "[99999999999999999999999]",
        ] {
            let error = KeyPath::parse(bad).unwrap_err();
            assert!(!error.to_string().is_empty(), "{bad}");
        }
    }

    #[test]
    fn lenient_parse_falls_back_to_one_key() {
        assert_eq!(
            KeyPath::from("a..b").segments(),
            [PathSegment::from("a..b")]
        );
        assert_eq!(KeyPath::from("a.b").len(), 2);
    }

    #[test]
    fn navigation() {
        let mut path: KeyPath = vec![PathSegment::from("a"), PathSegment::from(0usize)]
            .into_iter()
            .collect();
        assert_eq!(path.last().and_then(PathSegment::as_index), Some(0));
        assert_eq!(path.parent().unwrap().to_string(), "a");
        assert!(KeyPath::root().parent().is_none());
        assert!(path.parent().unwrap().is_prefix_of(&path));
        assert!(!path.is_prefix_of(&path.parent().unwrap()));
        assert_eq!(path.concat(&KeyPath::from("b.c")).to_string(), "a[0].b.c");
        assert_eq!(path.pop(), Some(PathSegment::Index(0)));
        assert_eq!(path.iter().count(), 1);
        assert_eq!(
            (&path).into_iter().next().and_then(PathSegment::as_key),
            Some("a")
        );
        assert!(!path.is_empty() && !path.is_root());
        assert_eq!(PathSegment::from("k").as_index(), None);
        assert_eq!(PathSegment::Index(1).as_key(), None);
        let from_static = KeyPath::from(crate::StaticKeyPath::new(&["x", "y"]));
        assert_eq!(from_static.to_string(), "x.y");
        assert_eq!("x.y".parse::<KeyPath>().unwrap(), from_static);
        let arc: alloc::sync::Arc<str> = alloc::sync::Arc::from("z");
        assert_eq!(PathSegment::from(arc).as_key(), Some("z"));
    }
}
