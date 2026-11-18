use core::fmt;

/// A numeric scalar: an unsigned integer, a negative integer, or a float.
///
/// Integers that fit `u64` are stored unsigned; negative integers as `i64`. A float is
/// never silently turned into an integer (a config field of type `u16` rejects `8080.0`).
#[derive(Clone, Copy, PartialEq)]
pub struct Number {
    repr: Repr,
}

#[derive(Clone, Copy, PartialEq)]
enum Repr {
    PosInt(u64),
    NegInt(i64),
    Float(f64),
}

impl Number {
    /// An unsigned integer.
    #[must_use]
    pub const fn from_u64(value: u64) -> Self {
        Self {
            repr: Repr::PosInt(value),
        }
    }

    /// A signed integer.
    #[must_use]
    pub const fn from_i64(value: i64) -> Self {
        if value < 0 {
            Self {
                repr: Repr::NegInt(value),
            }
        } else {
            #[allow(clippy::cast_sign_loss)]
            Self {
                repr: Repr::PosInt(value as u64),
            }
        }
    }

    /// A float.
    #[must_use]
    pub const fn from_f64(value: f64) -> Self {
        Self {
            repr: Repr::Float(value),
        }
    }

    /// Whether the number is a float.
    #[must_use]
    pub const fn is_float(&self) -> bool {
        matches!(self.repr, Repr::Float(_))
    }

    /// Whether the number is an integer.
    #[must_use]
    pub const fn is_integer(&self) -> bool {
        !self.is_float()
    }

    /// The value as `u64`, when it is a non-negative integer.
    #[must_use]
    pub const fn as_u64(&self) -> Option<u64> {
        match self.repr {
            Repr::PosInt(value) => Some(value),
            _ => None,
        }
    }

    /// The value as `i64`, when it is an integer in range.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self.repr {
            Repr::PosInt(value) => i64::try_from(value).ok(),
            Repr::NegInt(value) => Some(value),
            Repr::Float(_) => None,
        }
    }

    /// The value as `i128`, when it is an integer.
    #[must_use]
    pub fn as_i128(&self) -> Option<i128> {
        match self.repr {
            Repr::PosInt(value) => Some(i128::from(value)),
            Repr::NegInt(value) => Some(i128::from(value)),
            Repr::Float(_) => None,
        }
    }

    /// The value as `f64` (integers convert, possibly losing precision above 2^53).
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub const fn as_f64(&self) -> f64 {
        match self.repr {
            Repr::PosInt(value) => value as f64,
            Repr::NegInt(value) => value as f64,
            Repr::Float(value) => value,
        }
    }

    /// `"integer"` or `"float"`.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        if self.is_float() { "float" } else { "integer" }
    }
}

impl fmt::Debug for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.repr {
            Repr::PosInt(value) => write!(f, "{value}"),
            Repr::NegInt(value) => write!(f, "{value}"),
            Repr::Float(value) => {
                if value.is_finite() && value % 1.0 == 0.0 && value > -1e16 && value < 1e16 {
                    write!(f, "{value:.1}")
                } else {
                    write!(f, "{value}")
                }
            }
        }
    }
}

macro_rules! from_unsigned {
    ($($t:ty),*) => {$(
        impl From<$t> for Number {
            fn from(value: $t) -> Self {
                Self::from_u64(u64::from(value))
            }
        }
    )*};
}

macro_rules! from_signed {
    ($($t:ty),*) => {$(
        impl From<$t> for Number {
            fn from(value: $t) -> Self {
                Self::from_i64(i64::from(value))
            }
        }
    )*};
}

from_unsigned!(u8, u16, u32, u64);
from_signed!(i8, i16, i32, i64);

impl From<f64> for Number {
    fn from(value: f64) -> Self {
        Self::from_f64(value)
    }
}

impl From<f32> for Number {
    fn from(value: f32) -> Self {
        Self::from_f64(f64::from(value))
    }
}

#[cfg(test)]
mod tests {
    use alloc::format;

    use super::Number;

    #[test]
    fn representations() {
        let pos = Number::from(7u8);
        assert_eq!(pos.as_u64(), Some(7));
        assert_eq!(pos.as_i64(), Some(7));
        assert_eq!(pos.as_i128(), Some(7));
        assert!(pos.is_integer());
        assert_eq!(pos.kind_name(), "integer");

        let neg = Number::from(-3i32);
        assert_eq!(neg.as_u64(), None);
        assert_eq!(neg.as_i64(), Some(-3));
        assert_eq!(neg.as_i128(), Some(-3));
        assert!((neg.as_f64() + 3.0).abs() < f64::EPSILON);

        let big = Number::from_u64(u64::MAX);
        assert_eq!(big.as_i64(), None);
        assert_eq!(Number::from_i64(5), Number::from_u64(5));

        let float = Number::from(1.5f32);
        assert!(float.is_float());
        assert_eq!(float.as_i64(), None);
        assert_eq!(float.as_i128(), None);
        assert_eq!(float.as_u64(), None);
        assert_eq!(float.kind_name(), "float");
    }

    #[test]
    fn display() {
        assert_eq!(format!("{}", Number::from(8080u16)), "8080");
        assert_eq!(format!("{:?}", Number::from(-1i8)), "-1");
        assert_eq!(format!("{}", Number::from(2.0)), "2.0");
        assert_eq!(format!("{}", Number::from(0.25)), "0.25");
        assert_eq!(format!("{}", Number::from(f64::INFINITY)), "inf");
        assert_eq!(format!("{}", Number::from(1e20)), "100000000000000000000");
    }
}
