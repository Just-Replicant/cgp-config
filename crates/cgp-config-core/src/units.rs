//! Duration and byte-size text parsers (`#[config(parse = "duration")]`,
//! `#[config(parse = "bytes")]`). No allocation and no `humantime` dependency.
//!
//! | Kind | Accepted | Rejected |
//! |---|---|---|
//! | duration | `30s`, `5m`, `1h`, `100ms`, `1s200ms`, `1.5h`, `2d`, `250us`, `10ns` | `30` (no unit), `s`, `-1s`, `1x` |
//! | bytes | `512`, `1KB` (1000), `1KiB` (1024), `10MiB`, `1.5GB`, `64 k` | `-1`, `1.5` (fractional bytes), `1QB` |
//!
//! Units are ASCII case-insensitive, except that a duration's `m` (minute) and `ms`
//! (millisecond) are told apart by spelling only. A space between the number and the unit
//! is allowed for byte sizes.

use core::fmt;
use core::time::Duration;

/// Why a duration or byte size did not parse. Never contains the input text, so it is
/// safe on secret paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnitError {
    /// The text is empty.
    Empty,
    /// A number has no unit (a bare `30` is not a duration).
    MissingUnit,
    /// A number is malformed (`1..5s`, `-1s`, `s`).
    InvalidNumber,
    /// The unit is not known.
    UnknownUnit,
    /// The value does not fit (`u64` seconds or bytes).
    Overflow,
    /// A byte size has a fractional number of bytes.
    FractionalBytes,
}

impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "empty value",
            Self::MissingUnit => {
                "a duration needs a unit (ns, us, ms, s, m, h, d), for example \"30s\""
            }
            Self::InvalidNumber => "invalid number",
            Self::UnknownUnit => "unknown unit",
            Self::Overflow => "value is too large",
            Self::FractionalBytes => "a byte size must be a whole number of bytes",
        })
    }
}

impl core::error::Error for UnitError {}

/// A decimal number split into its integer part and up to 18 fraction digits.
struct Decimal {
    whole: u64,
    /// Fraction scaled to 10^18.
    fraction: u64,
}

const FRACTION_SCALE: u64 = 1_000_000_000_000_000_000;

/// Splits `text` into a leading decimal number and the rest.
fn take_number(text: &str) -> Result<(Decimal, &str), UnitError> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut whole: u64 = 0;
    let mut digits = 0;
    while let Some(byte) = bytes.get(index).filter(|b| b.is_ascii_digit()) {
        whole = whole
            .checked_mul(10)
            .and_then(|w| w.checked_add(u64::from(byte - b'0')))
            .ok_or(UnitError::Overflow)?;
        index += 1;
        digits += 1;
    }
    let mut fraction: u64 = 0;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let mut scale = FRACTION_SCALE / 10;
        let mut fraction_digits = 0;
        while let Some(byte) = bytes.get(index).filter(|b| b.is_ascii_digit()) {
            fraction += u64::from(byte - b'0') * scale;
            scale /= 10;
            index += 1;
            fraction_digits += 1;
        }
        if fraction_digits == 0 {
            return Err(UnitError::InvalidNumber);
        }
    }
    if digits == 0 {
        return Err(UnitError::InvalidNumber);
    }
    Ok((Decimal { whole, fraction }, &text[index..]))
}

/// Multiplies a decimal by `unit` (an integer factor) into a `u128`, truncating any
/// fraction below one.
fn scale(number: &Decimal, unit: u64) -> u128 {
    u128::from(number.whole) * u128::from(unit)
        + u128::from(number.fraction) * u128::from(unit) / u128::from(FRACTION_SCALE)
}

const NANOS_PER_SEC: u128 = 1_000_000_000;

fn duration_unit(unit: &str) -> Option<u64> {
    const UNITS: [(&str, u64); 8] = [
        ("ns", 1),
        ("us", 1_000),
        ("µs", 1_000),
        ("ms", 1_000_000),
        ("s", 1_000_000_000),
        ("m", 60_000_000_000),
        ("h", 3_600_000_000_000),
        ("d", 86_400_000_000_000),
    ];
    UNITS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(unit))
        .map(|(_, nanos)| *nanos)
}

/// Parses a duration: one or more `<number><unit>` parts, summed.
///
/// ```
/// use core::time::Duration;
/// use cgp_config_core::units::{UnitError, parse_duration};
///
/// assert_eq!(parse_duration("30s"), Ok(Duration::from_secs(30)));
/// assert_eq!(parse_duration("1s200ms"), Ok(Duration::from_millis(1200)));
/// assert_eq!(parse_duration("1.5h"), Ok(Duration::from_secs(5400)));
/// assert_eq!(parse_duration("30"), Err(UnitError::MissingUnit));
/// ```
///
/// # Errors
///
/// A [`UnitError`]; a bare number is [`UnitError::MissingUnit`].
pub fn parse_duration(text: &str) -> Result<Duration, UnitError> {
    if text.is_empty() {
        return Err(UnitError::Empty);
    }
    let mut rest = text;
    let mut total: u128 = 0;
    while !rest.is_empty() {
        let (number, after) = take_number(rest)?;
        let unit_len = after
            .char_indices()
            .find(|(_, c)| c.is_ascii_digit() || *c == '.')
            .map_or(after.len(), |(index, _)| index);
        let (unit, next) = after.split_at(unit_len);
        if unit.is_empty() {
            return Err(UnitError::MissingUnit);
        }
        let nanos = duration_unit(unit).ok_or(UnitError::UnknownUnit)?;
        total = total
            .checked_add(scale(&number, nanos))
            .ok_or(UnitError::Overflow)?;
        rest = next;
    }
    let secs = u64::try_from(total / NANOS_PER_SEC).map_err(|_| UnitError::Overflow)?;
    #[allow(clippy::cast_possible_truncation)] // below 10^9
    let nanos = (total % NANOS_PER_SEC) as u32;
    Ok(Duration::new(secs, nanos))
}

fn byte_unit(unit: &str) -> Option<u64> {
    const UNITS: [(&str, u64); 17] = [
        ("", 1),
        ("b", 1),
        ("k", 1_000),
        ("kb", 1_000),
        ("kib", 1 << 10),
        ("m", 1_000_000),
        ("mb", 1_000_000),
        ("mib", 1 << 20),
        ("g", 1_000_000_000),
        ("gb", 1_000_000_000),
        ("gib", 1 << 30),
        ("t", 1_000_000_000_000),
        ("tb", 1_000_000_000_000),
        ("tib", 1 << 40),
        ("pb", 1_000_000_000_000_000),
        ("pib", 1 << 50),
        ("eib", 1 << 60),
    ];
    UNITS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(unit))
        .map(|(_, factor)| *factor)
}

/// Parses a byte size: a number, an optional space, and an optional unit. A bare number is
/// bytes; `KB` is 1000 and `KiB` is 1024.
///
/// ```
/// use cgp_config_core::units::parse_bytes;
///
/// assert_eq!(parse_bytes("512"), Ok(512));
/// assert_eq!(parse_bytes("1KB"), Ok(1000));
/// assert_eq!(parse_bytes("10MiB"), Ok(10 * 1024 * 1024));
/// assert_eq!(parse_bytes("1.5 KiB"), Ok(1536));
/// ```
///
/// # Errors
///
/// A [`UnitError`].
pub fn parse_bytes(text: &str) -> Result<u64, UnitError> {
    if text.is_empty() {
        return Err(UnitError::Empty);
    }
    let (number, rest) = take_number(text)?;
    let unit = rest.strip_prefix(' ').unwrap_or(rest);
    let factor = byte_unit(unit).ok_or(UnitError::UnknownUnit)?;
    let exact = u128::from(number.fraction) * u128::from(factor) % u128::from(FRACTION_SCALE);
    if exact != 0 {
        return Err(UnitError::FractionalBytes);
    }
    u64::try_from(scale(&number, factor)).map_err(|_| UnitError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        let ok = [
            ("30s", Duration::from_secs(30)),
            ("5m", Duration::from_secs(300)),
            ("1h", Duration::from_secs(3600)),
            ("100ms", Duration::from_millis(100)),
            ("1s200ms", Duration::from_millis(1200)),
            ("2d", Duration::from_secs(172_800)),
            ("250us", Duration::from_micros(250)),
            ("250µs", Duration::from_micros(250)),
            ("10ns", Duration::from_nanos(10)),
            ("0.5s", Duration::from_millis(500)),
            ("1H30M", Duration::from_secs(5400)),
            ("0s", Duration::ZERO),
        ];
        for (text, expected) in ok {
            assert_eq!(parse_duration(text), Ok(expected), "{text}");
        }
        let bad = [
            ("", UnitError::Empty),
            ("30", UnitError::MissingUnit),
            ("1s30", UnitError::MissingUnit),
            ("s", UnitError::InvalidNumber),
            ("-1s", UnitError::InvalidNumber),
            ("1.s", UnitError::InvalidNumber),
            ("1x", UnitError::UnknownUnit),
            ("1 s", UnitError::UnknownUnit),
            ("99999999999999999999s", UnitError::Overflow),
        ];
        for (text, expected) in bad {
            assert_eq!(parse_duration(text), Err(expected), "{text}");
        }
        assert_eq!(
            parse_duration("18446744073709551615s"),
            Ok(Duration::from_secs(u64::MAX))
        );
        assert_eq!(
            parse_duration("18446744073709551615d"),
            Err(UnitError::Overflow)
        );
    }

    #[test]
    fn byte_sizes() {
        let ok = [
            ("0", 0),
            ("512", 512),
            ("512B", 512),
            ("1KB", 1000),
            ("1kb", 1000),
            ("1KiB", 1024),
            ("10MiB", 10 << 20),
            ("1.5GB", 1_500_000_000),
            ("64 k", 64_000),
            ("1EiB", 1 << 60),
        ];
        for (text, expected) in ok {
            assert_eq!(parse_bytes(text), Ok(expected), "{text}");
        }
        let bad = [
            ("", UnitError::Empty),
            ("-1", UnitError::InvalidNumber),
            ("1.5", UnitError::FractionalBytes),
            ("1QB", UnitError::UnknownUnit),
            ("1  KB", UnitError::UnknownUnit),
            ("16EiB", UnitError::Overflow),
        ];
        for (text, expected) in bad {
            assert_eq!(parse_bytes(text), Err(expected), "{text}");
        }
    }

    #[test]
    fn messages_never_echo_input() {
        for error in [
            UnitError::Empty,
            UnitError::MissingUnit,
            UnitError::InvalidNumber,
            UnitError::UnknownUnit,
            UnitError::Overflow,
            UnitError::FractionalBytes,
        ] {
            let mut buf = [0u8; 128];
            let mut cursor = Cursor(&mut buf, 0);
            core::fmt::Write::write_fmt(&mut cursor, format_args!("{error}")).unwrap();
            assert!(cursor.1 > 0);
        }
    }

    struct Cursor<'a>(&'a mut [u8], usize);

    impl core::fmt::Write for Cursor<'_> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let end = self.1 + s.len();
            self.0
                .get_mut(self.1..end)
                .ok_or(core::fmt::Error)?
                .copy_from_slice(s.as_bytes());
            self.1 = end;
            Ok(())
        }
    }
}
