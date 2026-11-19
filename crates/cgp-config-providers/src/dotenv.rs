//! The `DotEnv` layer (feature `dotenv`): `{PREFIX}__*` entries of a `.env` file.
//!
//! The file is parsed by this crate, hermetically: it never reads or writes the process
//! environment, and `${VAR}` inside values is **not** substituted (opt a field into
//! interpolation instead). Grammar:
//!
//! * `KEY=VALUE`, optionally prefixed by `export `; blank lines and `#` comments;
//! * unquoted values end at ` #` (a comment) and are trimmed;
//! * `'single'` quotes are literal; `"double"` quotes understand `\n`, `\r`, `\t`, `\"`,
//!   `\\`, and `\$`, and may span lines.
//!
//! Only `{PREFIX}__` names become config values, mapped like env variables
//! (`crate::env`). Origins are `Origin::DotEnv` (file and name).

use std::path::{Path, PathBuf};

use cgp_config_core::markers::DotEnv;
use cgp_config_core::{
    ConfigError, HasConfigValueType, HasEnvPrefix, HasLoadLimits, HasRootConfigMeta, LayerSkipped,
    Origin, SourceLoaded, SourceLoader, SourceLoaderComponent,
};
use cgp_config_value::ConfigValue;
use cgp_fork::prelude::*;
use zeroize::Zeroizing;

use crate::env::{SEPARATOR, map_entries};
use crate::read::{CanReadConfigFile, origin_of};

/// One `KEY=VALUE` entry and its 1-based line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotenvEntry {
    /// The name.
    pub key: String,
    /// The value, unquoted and unescaped.
    pub value: String,
    /// The line where the entry starts.
    pub line: usize,
}

/// Parses `.env` text.
///
/// ```
/// use cgp_config_providers::dotenv::parse_dotenv;
///
/// let entries = parse_dotenv("# db\nexport APP__DB__URL='pg://x'\nAPP__NAME=\"a\\tb\" # c\n").unwrap();
/// assert_eq!(entries[0].key, "APP__DB__URL");
/// assert_eq!(entries[0].value, "pg://x");
/// assert_eq!(entries[1].value, "a\tb");
/// ```
///
/// # Errors
///
/// A message naming the line (never its content) for a malformed entry.
pub fn parse_dotenv(text: &str) -> Result<Vec<DotenvEntry>, String> {
    let mut entries = Vec::new();
    let mut lines = text.lines().enumerate().peekable();
    while let Some((index, raw)) = lines.next() {
        let line_number = index + 1;
        let line = raw.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map_or(line, str::trim_start);
        let Some((key, rest)) = line.split_once('=') else {
            return Err(format!("line {line_number}: expected `KEY=VALUE`"));
        };
        let key = key.trim_end();
        let valid_key = !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-');
        if !valid_key {
            return Err(format!("line {line_number}: invalid variable name"));
        }
        let rest = rest.trim_start();
        let value = if let Some(body) = rest.strip_prefix('\'') {
            let end = body
                .find('\'')
                .ok_or_else(|| format!("line {line_number}: unterminated single quote"))?;
            check_trailing(&body[end + 1..], line_number)?;
            body[..end].to_owned()
        } else if let Some(body) = rest.strip_prefix('"') {
            let mut buffer = String::from(body);
            loop {
                if let Some((value, trailing)) = double_quoted(&buffer) {
                    check_trailing(trailing, line_number)?;
                    break value;
                }
                let Some((_, next)) = lines.next() else {
                    return Err(format!("line {line_number}: unterminated double quote"));
                };
                buffer.push('\n');
                buffer.push_str(next);
            }
        } else {
            let value = rest.find(" #").map_or(rest, |comment| &rest[..comment]);
            value.trim_end().to_owned()
        };
        entries.push(DotenvEntry {
            key: key.to_owned(),
            value,
            line: line_number,
        });
    }
    Ok(entries)
}

/// The value of a double-quoted body and the text after the closing quote, or `None`
/// when the quote is not closed yet.
fn double_quoted(body: &str) -> Option<(String, &str)> {
    let mut value = String::new();
    let mut chars = body.char_indices();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => return Some((value, &body[index + 1..])),
            '\\' => match chars.next() {
                Some((_, 'n')) => value.push('\n'),
                Some((_, 'r')) => value.push('\r'),
                Some((_, 't')) => value.push('\t'),
                Some((_, other @ ('"' | '\\' | '$'))) => value.push(other),
                Some((_, other)) => {
                    value.push('\\');
                    value.push(other);
                }
                None => return None,
            },
            other => value.push(other),
        }
    }
    None
}

fn check_trailing(trailing: &str, line: usize) -> Result<(), String> {
    let trailing = trailing.trim_start();
    if trailing.is_empty() || trailing.starts_with('#') {
        Ok(())
    } else {
        Err(format!(
            "line {line}: unexpected text after the closing quote"
        ))
    }
}

/// The `.env` file of a context. Implemented automatically for a context with a
/// `dotenv_path: Option<PathBuf>` field (`None` disables the layer).
#[cgp_auto_getter]
pub trait HasDotenvPath {
    /// The path.
    fn dotenv_path(&self) -> &Option<PathBuf>;
}

/// Loads `DotEnv` from `HasDotenvPath` through the context's file reader. Absent when no
/// path is set or the file does not exist.
pub struct LoadDotenv;

#[cgp_impl(LoadDotenv)]
impl SourceLoader<DotEnv>
where
    Self: HasConfigValueType<ConfigValue = ConfigValue>
        + CanRaiseError<ConfigError>
        + CanReadConfigFile
        + HasDotenvPath
        + HasEnvPrefix
        + HasRootConfigMeta
        + HasLoadLimits
        + CanLog<SourceLoaded>
        + CanLog<LayerSkipped>,
{
    fn load_source(
        &self,
        _source: PhantomData<DotEnv>,
    ) -> Result<Option<Self::ConfigValue>, Self::Error> {
        let Some(path) = self.dotenv_path().as_deref() else {
            return Ok(None);
        };
        let Some(file) = self.read_config_file(path)? else {
            self.log(LayerSkipped::new(origin_of(path)));
            return Ok(None);
        };
        let layer = dotenv_layer(path, &file.text, self.env_prefix(), self.root_config_meta())
            .map_err(Self::raise_error)?;
        if let Some(layer) = &layer {
            layer
                .check_limits(&self.load_limits())
                .map_err(Self::raise_error)?;
            self.log(SourceLoaded::new(
                file.origin.clone(),
                layer.count_entries(),
            ));
        }
        Ok(layer)
    }
}

/// The layer of `.env` `text` read from `path`.
///
/// # Errors
///
/// `ConfigError::Parse` for a malformed file, `AmbiguousEnv` as for env variables.
pub fn dotenv_layer(
    path: &Path,
    text: &str,
    prefix: &str,
    meta: Option<&'static cgp_config_core::ConfigMeta>,
) -> Result<Option<ConfigValue>, ConfigError> {
    let file_origin = origin_of(path);
    let entries = parse_dotenv(text)
        .map_err(|message| ConfigError::parse(file_origin.clone(), None, message))?;
    let entries: Vec<(String, Zeroizing<String>)> = entries
        .into_iter()
        .map(|entry| (entry.key, Zeroizing::new(entry.value)))
        .collect();
    let path_text = path.to_string_lossy();
    let marker = format!("{prefix}{SEPARATOR}");
    let layer = map_entries(
        entries.iter().map(|(key, value)| {
            (
                key.as_str(),
                value.as_str(),
                Origin::dotenv(path_text.as_ref(), key.as_str()),
            )
        }),
        &marker,
        meta,
    )?;
    crate::env::canonical(layer, meta)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{dotenv_layer, parse_dotenv};

    #[test]
    fn grammar() {
        let text = "\n# comment\nA=1\nexport B = two words # note\nC='lit $X # no'\n\
                    D=\"line1\\nline2 \\\"q\\\" \\$HOME\"\nE=\"multi\nline\"\nF=\nG=a#b\n";
        let entries = parse_dotenv(text).unwrap();
        let pairs: Vec<_> = entries
            .iter()
            .map(|e| (e.key.as_str(), e.value.as_str(), e.line))
            .collect();
        assert_eq!(
            pairs,
            [
                ("A", "1", 3),
                ("B", "two words", 4),
                ("C", "lit $X # no", 5),
                ("D", "line1\nline2 \"q\" $HOME", 6),
                ("E", "multi\nline", 7),
                ("F", "", 9),
                ("G", "a#b", 10),
            ]
        );
        for bad in ["NOVALUE", "=1", "A B=1", "A='open", "A=\"open", "A='x' y"] {
            let error = parse_dotenv(bad).unwrap_err();
            assert!(error.starts_with("line 1:"), "{bad}: {error}");
            assert!(!error.contains("open"), "never echoes content");
        }
    }

    #[test]
    fn only_prefixed_names_map() {
        let layer = dotenv_layer(
            Path::new(".env"),
            "APP__HTTP__PORT=1\nOTHER=2\nAPP_PROFILE=x\n",
            "APP",
            None,
        )
        .unwrap()
        .unwrap();
        let port = layer.get_path(&"http.port".into()).unwrap();
        assert_eq!(port.as_str(), Some("1"));
        assert_eq!(port.meta().origin.to_string(), ".env APP__HTTP__PORT");
        assert_eq!(layer.as_map().unwrap().len(), 1);
        let error = dotenv_layer(Path::new(".env"), "bad line", "APP", None).unwrap_err();
        assert_eq!(error.code(), "config::parse");
    }
}
