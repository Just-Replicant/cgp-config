//! TOML through the `toml` crate's spanned document parser.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

use cgp_config_core::{ConfigError, KeyPath, LoadLimits, Meta, Origin};
use cgp_config_value::{ConfigMap, ConfigValue, Kind, Number};
use toml::Spanned;
use toml::de::{DeTable, DeValue};

use crate::LineIndex;

/// Parses a TOML document.
///
/// ```
/// use cgp_config_core::{LoadLimits, Origin};
/// use cgp_config_formats::parse_toml;
///
/// let origin = Origin::file("app.toml");
/// let tree = parse_toml(&origin, "[http]\nport = 8080\n", &LoadLimits::DEFAULT)?;
/// let port = tree.get_path(&"http.port".into()).unwrap();
/// assert_eq!(port.as_u64(), Some(8080));
/// assert_eq!(port.meta().span.map(|s| (s.line, s.column)), Some((2, 8)));
///
/// let error = parse_toml(&origin, "a = 1\na = 2\n", &LoadLimits::DEFAULT).unwrap_err();
/// assert!(error.to_string().starts_with("app.toml:2:1: syntax error"), "{error}");
/// # Ok::<(), cgp_config_core::ConfigError>(())
/// ```
///
/// Datetimes become strings in their TOML text form (the value tree has no datetime
/// kind; deserialize them into a type that parses RFC 3339 text).
///
/// # Errors
///
/// `ConfigError::Parse` with `line:column` for invalid TOML, duplicate keys included;
/// `LimitExceeded` past `max_bytes`, `max_depth`, or `max_keys`.
pub fn parse_toml(
    origin: &Origin,
    text: &str,
    limits: &LoadLimits,
) -> Result<ConfigValue, ConfigError> {
    if text.len() > limits.max_bytes {
        return Err(ConfigError::limit_exceeded("max_bytes", limits.max_bytes));
    }
    let index = LineIndex::new(text);
    let document = DeTable::parse(text).map_err(|error| {
        let span = error.span().map(|range| index.span(range.start, range.end));
        ConfigError::parse(origin.clone(), span, error.message().trim_end())
    })?;
    let mut converter = Converter {
        origin,
        index: &index,
        limits,
        entries: 0,
    };
    let span = document.span();
    let root = converter.table(document.into_inner(), &mut KeyPath::root(), 0)?;
    Ok(ConfigValue::new(root, converter.meta(span.start, span.end)))
}

struct Converter<'a> {
    origin: &'a Origin,
    index: &'a LineIndex<'a>,
    limits: &'a LoadLimits,
    entries: usize,
}

impl Converter<'_> {
    fn meta(&self, start: usize, end: usize) -> Meta {
        Meta::new(self.origin.clone()).with_span(self.index.span(start, end))
    }

    fn admit(&mut self, depth: usize) -> Result<(), ConfigError> {
        if depth > self.limits.max_depth {
            return Err(ConfigError::limit_exceeded(
                "max_depth",
                self.limits.max_depth,
            ));
        }
        self.entries += 1;
        if self.entries > self.limits.max_keys {
            return Err(ConfigError::limit_exceeded(
                "max_keys",
                self.limits.max_keys,
            ));
        }
        Ok(())
    }

    fn table(
        &mut self,
        table: DeTable<'_>,
        path: &mut KeyPath,
        depth: usize,
    ) -> Result<Kind, ConfigError> {
        let mut map = ConfigMap::with_capacity(table.len());
        for (key, value) in table {
            self.admit(depth + 1)?;
            let key = key.into_inner();
            path.push(key.as_ref());
            let converted = self.value(value, path, depth + 1)?;
            path.pop();
            map.insert(Arc::<str>::from(key.as_ref()), converted);
        }
        Ok(Kind::Map(map))
    }

    fn value(
        &mut self,
        value: Spanned<DeValue<'_>>,
        path: &mut KeyPath,
        depth: usize,
    ) -> Result<ConfigValue, ConfigError> {
        let span = value.span();
        let meta = self.meta(span.start, span.end);
        let kind = match value.into_inner() {
            DeValue::String(text) => Kind::String(Arc::from(text.as_ref())),
            DeValue::Integer(integer) => Kind::Number(
                integer_number(integer.as_str(), integer.radix()).ok_or_else(|| {
                    ConfigError::parse(self.origin.clone(), meta.span, "integer out of range")
                })?,
            ),
            DeValue::Float(float) => {
                let text: String = float.as_str().chars().filter(|c| *c != '_').collect();
                let parsed = text.parse::<f64>().map_err(|_| {
                    ConfigError::parse(
                        self.origin.clone(),
                        meta.span,
                        format!("invalid float `{text}`"),
                    )
                })?;
                Kind::Number(Number::from_f64(parsed))
            }
            DeValue::Boolean(value) => Kind::Bool(value),
            DeValue::Datetime(datetime) => Kind::String(Arc::from(datetime.to_string())),
            DeValue::Array(array) => {
                let mut elements = Vec::with_capacity(array.len());
                for (position, element) in array.into_iter().enumerate() {
                    self.admit(depth + 1)?;
                    path.push(position);
                    elements.push(self.value(element, path, depth + 1)?);
                    path.pop();
                }
                Kind::Array(elements)
            }
            DeValue::Table(table) => self.table(table, path, depth)?,
        };
        Ok(ConfigValue::new(kind, meta))
    }
}

fn integer_number(digits: &str, radix: u32) -> Option<Number> {
    let digits: String = digits.chars().filter(|c| *c != '_').collect();
    let (negative, body) = match digits.strip_prefix('-') {
        Some(body) => (true, body),
        None => (false, digits.strip_prefix('+').unwrap_or(&digits)),
    };
    if negative {
        i64::from_str_radix(&digits, radix)
            .ok()
            .map(Number::from_i64)
    } else {
        u64::from_str_radix(body, radix).ok().map(Number::from_u64)
    }
}

/// Renders a redacted copy of `value` as TOML.
///
/// # Errors
///
/// `ConfigError::Parse` when the tree has no TOML form (a null, or a root that is not a
/// table).
pub fn render_toml(value: &ConfigValue) -> Result<String, ConfigError> {
    toml::to_string_pretty(&value.redacted()).map_err(|error| {
        ConfigError::parse(
            Origin::Unknown,
            None,
            format!("cannot render TOML: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use cgp_config_core::{LoadLimits, Origin};
    use cgp_config_value::ConfigValue;

    use super::{parse_toml, render_toml};

    fn parse(text: &str) -> Result<ConfigValue, cgp_config_core::ConfigError> {
        parse_toml(&Origin::file("t.toml"), text, &LoadLimits::DEFAULT)
    }

    #[test]
    fn scalars_and_containers() {
        let tree = parse(
            "i = -42\nh = 0xff\no = 0o17\nb = 0b101\nbig = 18446744073709551615\nu = 1_000\n\
             f = 6.5e-1\ninf = inf\nt = true\nd = 1979-05-27T07:32:00Z\narr = [1, \"x\"]\n\
             [t2]\ninline = { a = 1 }\n[[aot]]\nn = 1\n[[aot]]\nn = 2\n",
        )
        .unwrap();
        assert_eq!(tree.get("i").unwrap().as_i64(), Some(-42));
        assert_eq!(tree.get("h").unwrap().as_u64(), Some(255));
        assert_eq!(tree.get("o").unwrap().as_u64(), Some(15));
        assert_eq!(tree.get("b").unwrap().as_u64(), Some(5));
        assert_eq!(tree.get("big").unwrap().as_u64(), Some(u64::MAX));
        assert_eq!(tree.get("u").unwrap().as_u64(), Some(1000));
        assert_eq!(tree.get("f").unwrap().as_f64(), Some(0.65));
        assert_eq!(tree.get("inf").unwrap().as_f64(), Some(f64::INFINITY));
        assert_eq!(tree.get("t").unwrap().as_bool(), Some(true));
        assert_eq!(
            tree.get("d").unwrap().as_str(),
            Some("1979-05-27T07:32:00Z")
        );
        assert_eq!(
            tree.get_path(&"t2.inline.a".into()).unwrap().as_u64(),
            Some(1)
        );
        assert_eq!(tree.get_path(&"aot[1].n".into()).unwrap().as_u64(), Some(2));
        assert_eq!(tree.get_path(&"arr[1]".into()).unwrap().as_str(), Some("x"));
    }

    #[test]
    fn errors_and_limits() {
        let error = parse("[a]\nx = 1\n[a]\n").unwrap_err();
        assert_eq!(error.code(), "config::parse");
        assert!(error.to_string().starts_with("t.toml:3:"), "{error}");
        assert!(parse("x = ").is_err());
        let limits = LoadLimits::DEFAULT.with_max_keys(2);
        let error = parse_toml(&Origin::Unknown, "a = 1\nb = 2\nc = 3\n", &limits).unwrap_err();
        assert_eq!(error.code(), "config::limit_exceeded");
        let limits = LoadLimits::DEFAULT.with_max_depth(2);
        let error = parse_toml(&Origin::Unknown, "a = [[[1]]]", &limits).unwrap_err();
        assert_eq!(error.code(), "config::limit_exceeded");
    }

    #[test]
    fn renders_redacted() {
        let mut tree = parse("password = \"hunter2\"\nport = 1\n").unwrap();
        tree.as_map_mut()
            .unwrap()
            .get_mut("password")
            .unwrap()
            .meta_mut()
            .secret = true;
        let text = render_toml(&tree).unwrap();
        assert!(!text.contains("hunter2"));
        assert!(text.contains("port = 1"));
        let reparsed = parse(&text).unwrap();
        assert_eq!(reparsed.get("port").unwrap().as_u64(), Some(1));
        assert!(render_toml(&ConfigValue::map([("n", ConfigValue::null())])).is_err());
    }
}
