//! [`ConfigDiagnostic`]: a [`miette::Diagnostic`] over [`ConfigError`].
//!
//! Spans on secret nodes are omitted, and the cached source has those spans blanked, so a
//! snippet cannot show secret bytes. Contexts that use `anyhow` as their error type still
//! load; `anyhow::Error` does not implement `Diagnostic`, so the labels are dropped.

use std::fmt;

use cgp_config_core::{ConfigError, Origin, PathSegment};
use miette::{Diagnostic, LabeledSpan, NamedSource, SourceSpan};

/// A [`ConfigError`] with optional source text for labeled spans.
pub struct ConfigDiagnostic {
    error: ConfigError,
    source: Option<NamedSource<String>>,
    /// Source with secret spans overwritten by spaces. Not shown for secret errors.
    blanked: String,
    label: Option<SourceSpan>,
    help: Option<String>,
    env_prefix: Option<String>,
}

impl ConfigDiagnostic {
    /// Wraps `error`. Add source text with [`with_source`](Self::with_source).
    #[must_use]
    pub fn new(error: ConfigError) -> Self {
        let help = help_text(&error, None);
        Self {
            error,
            source: None,
            blanked: String::new(),
            label: None,
            help,
            env_prefix: None,
        }
    }

    /// The underlying error.
    #[must_use]
    pub fn error(&self) -> &ConfigError {
        &self.error
    }

    /// Prefix for the help line's env name (`APP` produces `APP__HTTP__PORT`).
    #[must_use]
    pub fn with_env_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.env_prefix = Some(prefix.into());
        self.help = help_text(&self.error, self.env_prefix.as_deref());
        self
    }

    /// Attaches source text. A secret span is blanked and not labeled.
    #[must_use]
    pub fn with_source(mut self, name: impl Into<String>, source: impl Into<String>) -> Self {
        let source = source.into();
        self.blanked = blank_secret(&source, &self.error);
        let secret = self.error.meta().is_some_and(|meta| meta.secret);
        if !secret {
            if let Some(span) = self.error.meta().and_then(|meta| meta.span) {
                let len = span.end.saturating_sub(span.start);
                self.label = Some(SourceSpan::from((span.start, len)));
            }
            self.source = Some(NamedSource::new(name.into(), self.blanked.clone()));
        }
        self
    }
}

impl fmt::Debug for ConfigDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigDiagnostic")
            .field("error", &self.error)
            .field("help", &self.help)
            .finish()
    }
}

impl fmt::Display for ConfigDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.error, f)
    }
}

impl std::error::Error for ConfigDiagnostic {}

impl Diagnostic for ConfigDiagnostic {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(self.error.code()))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.help
            .as_ref()
            .map(|help| Box::new(help.clone()) as Box<dyn fmt::Display>)
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        self.source
            .as_ref()
            .map(|source| source as &dyn miette::SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let label = self.label?;
        Some(Box::new(std::iter::once(LabeledSpan::at(label, "here"))))
    }
}

fn blank_secret(source: &str, error: &ConfigError) -> String {
    let Some(meta) = error.meta() else {
        return source.to_owned();
    };
    if !meta.secret {
        return source.to_owned();
    }
    let Some(span) = meta.span else {
        return source.to_owned();
    };
    let mut bytes = source.as_bytes().to_vec();
    let end = span.end.min(bytes.len());
    let start = span.start.min(end);
    for byte in &mut bytes[start..end] {
        *byte = b' ';
    }
    String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

fn help_text(error: &ConfigError, prefix: Option<&str>) -> Option<String> {
    let path = error.path()?;
    let keys: Vec<&str> = path
        .segments()
        .iter()
        .filter_map(PathSegment::as_key)
        .collect();
    let key = *keys.last()?;
    let mut parts = Vec::new();
    if let Some(prefix) = prefix {
        parts.push(prefix.to_owned());
    }
    parts.extend(keys.iter().map(|key| key.to_ascii_uppercase()));
    let env = parts.join("__");
    let under = if keys.len() == 1 {
        String::from("[the document root]")
    } else {
        format!("[{}]", keys[..keys.len() - 1].join("."))
    };
    let file = match error.meta().map(|meta| &meta.origin) {
        Some(Origin::File { path }) => format!(" in {path}"),
        _ => String::new(),
    };
    Some(format!("set {env} or add `{key}` under {under}{file}"))
}

#[cfg(test)]
mod tests {
    use cgp_config_core::{ConfigError, KeyPath, Meta, Origin, Span};
    use miette::Diagnostic;

    use super::ConfigDiagnostic;

    #[test]
    fn help_names_the_env_var_and_the_file_key() {
        let error = ConfigError::type_error(
            KeyPath::parse("http.port").unwrap(),
            "expected u16",
            Some(Meta::new(Origin::file("config/prod.toml")).with_span(Span::new(10, 14, 2, 3))),
        );
        let diagnostic = ConfigDiagnostic::new(error)
            .with_env_prefix("APP")
            .with_source("config/prod.toml", "port = 8080\n");
        let help = diagnostic.help.clone().unwrap();
        assert_eq!(
            help,
            "set APP__HTTP__PORT or add `port` under [http] in config/prod.toml"
        );
        assert!(diagnostic.labels().is_some());
    }

    #[test]
    fn a_secret_span_has_no_snippet_and_the_source_is_blanked() {
        let source = "password = \"s3cr3t-SENTINEL\"";
        let start = source.find("s3cr3t").unwrap();
        let error = ConfigError::type_error(
            KeyPath::from("password"),
            "leaked s3cr3t-SENTINEL",
            Some(
                Meta::new(Origin::file("app.toml"))
                    .with_span(Span::new(start, start + "s3cr3t-SENTINEL".len(), 1, 12))
                    .with_secret(true),
            ),
        );
        let diagnostic = ConfigDiagnostic::new(error).with_source("app.toml", source);
        assert!(diagnostic.source_code().is_none());
        assert!(diagnostic.labels().is_none());
        assert!(
            !diagnostic.blanked.contains("SENTINEL"),
            "{}",
            diagnostic.blanked
        );
        assert!(!diagnostic.to_string().contains("SENTINEL"));
        assert!(!format!("{diagnostic:?}").contains("SENTINEL"));
    }
}
