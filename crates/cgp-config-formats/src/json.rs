//! A strict JSON (RFC 8259) parser for configuration documents.
//!
//! Compared with a general-purpose parser it keeps what configuration needs: a span on
//! every node, duplicate keys rejected with their path and position, and depth and key
//! limits checked *while* parsing, so a hostile document cannot allocate or recurse past
//! them. Comments, trailing commas, `NaN`, and single quotes are errors, as in the RFC.

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use cgp_config_core::{ConfigError, KeyPath, LoadLimits, Meta, Origin};
use cgp_config_value::{ConfigMap, ConfigValue, Kind, Number};

use crate::LineIndex;

/// Parses `text` as one JSON value.
///
/// ```
/// use cgp_config_core::{LoadLimits, Origin};
/// use cgp_config_formats::parse_json;
///
/// let origin = Origin::file("app.json");
/// let tree = parse_json(&origin, r#"{"http": {"port": 8080}}"#, &LoadLimits::DEFAULT)?;
/// let port = tree.get_path(&"http.port".into()).unwrap();
/// assert_eq!(port.as_u64(), Some(8080));
/// assert_eq!(port.meta().span.map(|s| (s.line, s.column)), Some((1, 19)));
///
/// let error = parse_json(&origin, "{\"a\": 1,\n \"a\": 2}", &LoadLimits::DEFAULT).unwrap_err();
/// assert_eq!(error.code(), "config::duplicate_key");
/// assert!(error.to_string().contains("app.json:2:2"));
/// # Ok::<(), cgp_config_core::ConfigError>(())
/// ```
///
/// # Errors
///
/// `ConfigError::Parse` with the position for invalid JSON, `DuplicateKey` for a key
/// repeated in one object, `LimitExceeded` past `max_bytes`, `max_depth`, or `max_keys`.
pub fn parse_json(
    origin: &Origin,
    text: &str,
    limits: &LoadLimits,
) -> Result<ConfigValue, ConfigError> {
    if text.len() > limits.max_bytes {
        return Err(ConfigError::limit_exceeded("max_bytes", limits.max_bytes));
    }
    let mut parser = Parser {
        bytes: text.as_bytes(),
        text,
        pos: 0,
        index: LineIndex::new(text),
        origin,
        limits,
        entries: 0,
        path: KeyPath::root(),
    };
    parser.skip_whitespace();
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.pos < parser.bytes.len() {
        return Err(parser.error_at(parser.pos, "trailing characters after the document"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    pos: usize,
    index: LineIndex<'a>,
    origin: &'a Origin,
    limits: &'a LoadLimits,
    /// Map entries and array elements admitted so far.
    entries: usize,
    path: KeyPath,
}

impl Parser<'_> {
    fn error_at(&self, at: usize, message: &str) -> ConfigError {
        ConfigError::parse(
            self.origin.clone(),
            Some(self.index.span(at, (at + 1).min(self.bytes.len()))),
            message,
        )
    }

    fn meta(&self, start: usize, end: usize) -> Meta {
        Meta::new(self.origin.clone()).with_span(self.index.span(start, end))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.pos += 1;
        }
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

    fn value(&mut self, depth: usize) -> Result<ConfigValue, ConfigError> {
        let start = self.pos;
        let kind = match self.peek() {
            Some(b'{') => self.object(depth)?,
            Some(b'[') => self.array(depth)?,
            Some(b'"') => Kind::String(Arc::from(self.string()?)),
            Some(b't') => self.literal("true", Kind::Bool(true))?,
            Some(b'f') => self.literal("false", Kind::Bool(false))?,
            Some(b'n') => self.literal("null", Kind::Null)?,
            Some(b'-' | b'0'..=b'9') => Kind::Number(self.number()?),
            Some(_) => return Err(self.error_at(start, "expected a JSON value")),
            None => {
                return Err(self.error_at(start, "unexpected end of document, expected a value"));
            }
        };
        Ok(ConfigValue::new(kind, self.meta(start, self.pos)))
    }

    fn literal(&mut self, word: &str, kind: Kind) -> Result<Kind, ConfigError> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(kind)
        } else {
            Err(self.error_at(self.pos, "expected a JSON value"))
        }
    }

    fn object(&mut self, depth: usize) -> Result<Kind, ConfigError> {
        self.pos += 1;
        let mut map = ConfigMap::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Kind::Map(map));
        }
        loop {
            self.skip_whitespace();
            let key_start = self.pos;
            if self.peek() != Some(b'"') {
                return Err(self.error_at(key_start, "expected a string key"));
            }
            let key = self.string()?;
            let key_end = self.pos;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err(self.error_at(self.pos, "expected `:` after the key"));
            }
            self.pos += 1;
            self.admit(depth + 1)?;
            self.path.push(key.as_str());
            if map.contains_key(&key) {
                let error =
                    ConfigError::duplicate_key(self.path.clone(), self.meta(key_start, key_end));
                return Err(error);
            }
            self.skip_whitespace();
            let value = self.value(depth + 1)?;
            self.path.pop();
            map.insert(key, value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Kind::Map(map));
                }
                _ => return Err(self.error_at(self.pos, "expected `,` or `}` in an object")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Kind, ConfigError> {
        self.pos += 1;
        let mut elements = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Kind::Array(elements));
        }
        loop {
            self.skip_whitespace();
            self.admit(depth + 1)?;
            self.path.push(elements.len());
            let value = self.value(depth + 1)?;
            self.path.pop();
            elements.push(value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Kind::Array(elements));
                }
                _ => return Err(self.error_at(self.pos, "expected `,` or `]` in an array")),
            }
        }
    }

    /// A string starting at the opening quote; leaves `pos` after the closing quote.
    fn string(&mut self) -> Result<String, ConfigError> {
        let open = self.pos;
        self.pos += 1;
        let mut out = String::new();
        let mut run_start = self.pos;
        loop {
            let Some(byte) = self.peek() else {
                return Err(self.error_at(open, "unterminated string"));
            };
            match byte {
                b'"' => {
                    out.push_str(&self.text[run_start..self.pos]);
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(&self.text[run_start..self.pos]);
                    self.escape(&mut out)?;
                    run_start = self.pos;
                }
                0x00..=0x1f => {
                    return Err(self.error_at(self.pos, "control character in a string"));
                }
                _ => self.pos += 1,
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), ConfigError> {
        let at = self.pos;
        self.pos += 1;
        let Some(byte) = self.peek() else {
            return Err(self.error_at(at, "unterminated escape"));
        };
        self.pos += 1;
        match byte {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{8}'),
            b'f' => out.push('\u{c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let first = self.hex4(at)?;
                let scalar = if (0xD800..0xDC00).contains(&first) {
                    if !self.bytes[self.pos..].starts_with(b"\\u") {
                        return Err(self.error_at(at, "unpaired surrogate in a \\u escape"));
                    }
                    self.pos += 2;
                    let second = self.hex4(at)?;
                    if !(0xDC00..0xE000).contains(&second) {
                        return Err(self.error_at(at, "unpaired surrogate in a \\u escape"));
                    }
                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                } else if (0xDC00..0xE000).contains(&first) {
                    return Err(self.error_at(at, "unpaired surrogate in a \\u escape"));
                } else {
                    first
                };
                out.push(
                    char::from_u32(scalar)
                        .ok_or_else(|| self.error_at(at, "invalid \\u escape"))?,
                );
            }
            _ => return Err(self.error_at(at, "invalid escape")),
        }
        Ok(())
    }

    fn hex4(&mut self, at: usize) -> Result<u32, ConfigError> {
        let digits = self
            .text
            .get(self.pos..self.pos + 4)
            .filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| self.error_at(at, "expected four hex digits in a \\u escape"))?;
        self.pos += 4;
        u32::from_str_radix(digits, 16).map_err(|_| self.error_at(at, "invalid \\u escape"))
    }

    fn number(&mut self) -> Result<Number, ConfigError> {
        let start = self.pos;
        let negative = self.peek() == Some(b'-');
        if negative {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.error_at(start, "invalid number")),
        }
        let mut integer = true;
        if self.peek() == Some(b'.') {
            integer = false;
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error_at(start, "invalid number: digits expected after `.`"));
            }
            self.digits();
        }
        if let Some(b'e' | b'E') = self.peek() {
            integer = false;
            self.pos += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error_at(start, "invalid number: digits expected in the exponent"));
            }
            self.digits();
        }
        let text = &self.text[start..self.pos];
        if integer {
            let parsed = if negative {
                text.parse::<i64>().ok().map(Number::from_i64)
            } else {
                text.parse::<u64>().ok().map(Number::from_u64)
            };
            if let Some(number) = parsed {
                return Ok(number);
            }
        }
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => Ok(Number::from_f64(value)),
            _ => Err(self.error_at(start, &format!("number out of range: {text}"))),
        }
    }

    fn digits(&mut self) {
        while let Some(b'0'..=b'9') = self.peek() {
            self.pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use cgp_config_core::{LoadLimits, Origin};

    use super::parse_json;

    fn parse(text: &str) -> Result<cgp_config_value::ConfigValue, cgp_config_core::ConfigError> {
        parse_json(&Origin::file("t.json"), text, &LoadLimits::DEFAULT)
    }

    #[test]
    fn values() {
        let tree = parse(
            r#" {"s": "a\"b\\\/\b\f\n\r\t\u00e9\ud83d\ude00", "n": [0, -1, 1.5, 2e3, 18446744073709551615, -9223372036854775808, 99999999999999999999], "b": [true, false, null], "e": {}, "a": []} "#,
        )
        .unwrap();
        assert_eq!(
            tree.get("s").unwrap().as_str(),
            Some("a\"b\\/\u{8}\u{c}\n\r\té😀")
        );
        let n = tree.get("n").unwrap().as_array().unwrap();
        assert_eq!(n[0].as_u64(), Some(0));
        assert_eq!(n[1].as_i64(), Some(-1));
        assert_eq!(n[2].as_f64(), Some(1.5));
        assert_eq!(n[3].as_f64(), Some(2000.0));
        assert_eq!(n[4].as_u64(), Some(u64::MAX));
        assert_eq!(n[5].as_i64(), Some(i64::MIN));
        assert_eq!(n[6].as_f64(), Some(1e20));
        assert!(tree.get("b").unwrap().as_array().unwrap()[2].is_null());
    }

    #[test]
    fn rejects_what_the_rfc_rejects() {
        for bad in [
            "",
            "{",
            "}",
            "[1,]",
            "{\"a\":1,}",
            "{'a':1}",
            "{a:1}",
            "01",
            "1.",
            ".5",
            "-",
            "1e",
            "+1",
            "NaN",
            "Infinity",
            "tru",
            "nul",
            "\"\\x\"",
            "\"\\u12\"",
            "\"\\ud800\"",
            "\"\\udc00\"",
            "\"\\ud800\\u0041\"",
            "\"a\nb\"",
            "\"open",
            "{} {}",
            "// c\n{}",
            "[1 2]",
            "{\"a\" 1}",
            "1e400",
        ] {
            let error = parse(bad).unwrap_err();
            assert_eq!(error.code(), "config::parse", "{bad:?}: {error}");
        }
    }

    #[test]
    fn errors_carry_positions_and_duplicates_paths() {
        let error = parse("{\n  \"a\": [1,\n    }").unwrap_err();
        assert!(
            error.to_string().starts_with("t.json:3:5: syntax error"),
            "{error}"
        );
        let error = parse("{\"a\": {\"b\": 1, \"b\": 2}}").unwrap_err();
        assert_eq!(error.code(), "config::duplicate_key");
        assert_eq!(error.path().unwrap().to_string(), "a.b");
    }

    #[test]
    fn limits_hold_while_parsing() {
        let deep = "[".repeat(10) + &"]".repeat(10);
        let limits = LoadLimits::DEFAULT.with_max_depth(5);
        let error = parse_json(&Origin::Unknown, &deep, &limits).unwrap_err();
        assert_eq!(error.code(), "config::limit_exceeded");
        let wide = "[1,2,3,4]";
        let limits = LoadLimits::DEFAULT.with_max_keys(3);
        let error = parse_json(&Origin::Unknown, wide, &limits).unwrap_err();
        assert_eq!(error.code(), "config::limit_exceeded");
        assert!(parse_json(&Origin::Unknown, "[1,2,3]", &limits).is_ok());
    }
}
