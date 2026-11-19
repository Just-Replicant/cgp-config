use alloc::sync::Arc;
use core::fmt;

/// Where a value came from.
///
/// Origins name sources, never values: an env origin carries the variable *name*, a file
/// origin the path. They are safe to log.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Origin {
    /// Not recorded (values built in code without provenance).
    #[default]
    Unknown,
    /// Const defaults from the config type's metadata.
    Defaults,
    /// A file, by the path it was read from.
    File {
        /// The path as given to the reader.
        path: Arc<str>,
    },
    /// One environment variable, by name.
    Env {
        /// The variable name, such as `APP__HTTP__PORT`.
        key: Arc<str>,
    },
    /// A single environment variable holding a whole JSON document (`APP_DOCUMENT`).
    EnvDocument {
        /// The variable name.
        key: Arc<str>,
    },
    /// One variable of a `.env` file (the `dotenv` feature), by file and name. Shared
    /// behind one `Arc` to keep `Origin` (and so every error) small.
    DotEnv(Arc<DotEnvSource>),
    /// A command-line override the application already parsed.
    Cli {
        /// The argument or override key, such as `--http.port`.
        arg: Arc<str>,
    },
    /// A document compiled into the binary (`include_str!`).
    Embedded {
        /// A label for the document.
        name: Arc<str>,
    },
    /// An in-memory layer supplied by code or a test.
    Memory {
        /// A label for the layer.
        name: Arc<str>,
    },
    /// A source outside this crate (a satellite crate or an application source).
    Custom {
        /// A label that identifies the source in messages.
        label: Arc<str>,
    },
}

/// The file and variable of an [`Origin::DotEnv`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct DotEnvSource {
    /// The `.env` file.
    pub path: Arc<str>,
    /// The variable name.
    pub key: Arc<str>,
}

impl Origin {
    /// A file origin.
    #[must_use]
    pub fn file(path: impl Into<Arc<str>>) -> Self {
        Self::File { path: path.into() }
    }

    /// An environment variable origin.
    #[must_use]
    pub fn env(key: impl Into<Arc<str>>) -> Self {
        Self::Env { key: key.into() }
    }

    /// An env-document origin.
    #[must_use]
    pub fn env_document(key: impl Into<Arc<str>>) -> Self {
        Self::EnvDocument { key: key.into() }
    }

    /// A `.env` variable origin.
    #[must_use]
    pub fn dotenv(path: impl Into<Arc<str>>, key: impl Into<Arc<str>>) -> Self {
        Self::DotEnv(Arc::new(DotEnvSource {
            path: path.into(),
            key: key.into(),
        }))
    }

    /// A command-line origin.
    #[must_use]
    pub fn cli(arg: impl Into<Arc<str>>) -> Self {
        Self::Cli { arg: arg.into() }
    }

    /// An embedded-document origin.
    #[must_use]
    pub fn embedded(name: impl Into<Arc<str>>) -> Self {
        Self::Embedded { name: name.into() }
    }

    /// An in-memory origin.
    #[must_use]
    pub fn memory(name: impl Into<Arc<str>>) -> Self {
        Self::Memory { name: name.into() }
    }

    /// A custom origin.
    #[must_use]
    pub fn custom(label: impl Into<Arc<str>>) -> Self {
        Self::Custom {
            label: label.into(),
        }
    }

    /// Whether the origin is [`Origin::Unknown`].
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Short kind name, stable for tracing fields: `unknown`, `defaults`, `file`, `env`,
    /// `env-document`, `dotenv`, `cli`, `embedded`, `memory`, `custom`.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Defaults => "defaults",
            Self::File { .. } => "file",
            Self::Env { .. } => "env",
            Self::EnvDocument { .. } => "env-document",
            Self::DotEnv(_) => "dotenv",
            Self::Cli { .. } => "cli",
            Self::Embedded { .. } => "embedded",
            Self::Memory { .. } => "memory",
            Self::Custom { .. } => "custom",
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => f.write_str("unknown origin"),
            Self::Defaults => f.write_str("defaults"),
            Self::File { path } => f.write_str(path),
            Self::Env { key } => write!(f, "env {key}"),
            Self::EnvDocument { key } => write!(f, "env document {key}"),
            Self::DotEnv(source) => write!(f, "{} {}", source.path, source.key),
            Self::Cli { arg } => write!(f, "cli {arg}"),
            Self::Embedded { name } => write!(f, "embedded {name}"),
            Self::Memory { name } => write!(f, "memory {name}"),
            Self::Custom { label } => f.write_str(label),
        }
    }
}

/// A region of source text: a byte range plus the 1-based line and column of its start.
///
/// `Display` writes `line:column`, so `"{origin}:{span}"` reads `config/prod.toml:12:8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    /// Byte offset of the first byte.
    pub start: usize,
    /// Byte offset one past the last byte.
    pub end: usize,
    /// 1-based line of `start`.
    pub line: u32,
    /// 1-based column (in characters) of `start`.
    pub column: u32,
}

impl Span {
    /// A span from its parts.
    ///
    /// An `end` before `start` is clamped to `start` (an empty span), the same rule
    /// [`from_offsets`](Self::from_offsets) applies, so [`len`](Self::len) never underflows.
    #[must_use]
    pub const fn new(start: usize, end: usize, line: u32, column: u32) -> Self {
        Self {
            start,
            end: if end < start { start } else { end },
            line,
            column,
        }
    }

    /// A span over `start..end` of `text`, computing line and column.
    ///
    /// `start` is clamped to `text.len()` and moved back to a character boundary.
    #[must_use]
    pub fn from_offsets(text: &str, start: usize, end: usize) -> Self {
        let mut start = start.min(text.len());
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        let before = &text[..start];
        let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |p| p + 1);
        let column = before[line_start..].chars().count() + 1;
        Self {
            start,
            end: end.max(start),
            line: u32::try_from(line).unwrap_or(u32::MAX),
            column: u32::try_from(column).unwrap_or(u32::MAX),
        }
    }

    /// Length in bytes. Zero for a span whose public fields were set with `end < start`.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the span covers no bytes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::{Origin, Span};

    #[test]
    fn display_names_the_source_not_the_value() {
        let cases = [
            (Origin::Unknown, "unknown origin", "unknown"),
            (Origin::Defaults, "defaults", "defaults"),
            (Origin::file("config/prod.toml"), "config/prod.toml", "file"),
            (Origin::env("APP__HTTP__PORT"), "env APP__HTTP__PORT", "env"),
            (
                Origin::env_document("APP_DOCUMENT"),
                "env document APP_DOCUMENT",
                "env-document",
            ),
            (Origin::dotenv(".env", "APP__X"), ".env APP__X", "dotenv"),
            (Origin::cli("--http.port"), "cli --http.port", "cli"),
            (
                Origin::embedded("defaults.toml"),
                "embedded defaults.toml",
                "embedded",
            ),
            (Origin::memory("test"), "memory test", "memory"),
            (Origin::custom("vault kv/app"), "vault kv/app", "custom"),
        ];
        for (origin, text, kind) in cases {
            assert_eq!(origin.to_string(), text);
            assert_eq!(origin.kind(), kind);
        }
        assert!(Origin::default().is_unknown());
        assert!(!Origin::Defaults.is_unknown());
    }

    #[test]
    fn span_from_offsets() {
        let text = "a = 1\nport = \"x\"\nü = 2";
        let span = Span::from_offsets(text, 13, 16);
        assert_eq!((span.line, span.column), (2, 8));
        assert_eq!(span.to_string(), "2:8");
        assert_eq!(span.len(), 3);
        assert!(!span.is_empty());
        // after a multi-byte character, columns count characters
        let after_u = text.find('=').map(|_| text.rfind('=').unwrap()).unwrap();
        assert_eq!(Span::from_offsets(text, after_u, after_u).column, 3);
        // clamped and moved to a char boundary
        let clamped = Span::from_offsets(text, 1000, 0);
        assert_eq!(clamped.start, text.len());
        assert!(clamped.is_empty());
        let inside = text.find('ü').unwrap() + 1;
        assert_eq!(Span::from_offsets(text, inside, inside).start, inside - 1);
        assert_eq!(Span::new(0, 1, 1, 1).len(), 1);
    }

    #[test]
    fn inverted_spans_do_not_panic() {
        // `new` clamps `end` to `start`, like `from_offsets`.
        let span = Span::new(10, 3, 1, 1);
        assert_eq!((span.start, span.end), (10, 10));
        assert_eq!(span.len(), 0);
        assert!(span.is_empty());
        // The fields are public, so `len` also saturates on a hand-built inverted span.
        let raw = Span {
            start: 10,
            end: 3,
            line: 1,
            column: 1,
        };
        assert_eq!(raw.len(), 0);
        assert!(raw.is_empty());
    }
}
