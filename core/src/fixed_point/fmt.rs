//! Display and Debug formatting for `FixedPoint`.

use super::FixedPoint;
use core::fmt;

impl<const D: u32> fmt::Debug for FixedPoint<u64, D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FixedPoint<u64, {}>(raw={})", D, self.raw)
    }
}

impl<const D: u32> fmt::Display for FixedPoint<u64, D> {
    /// Formats as `integer.fractional` with exactly `DECIMALS` fractional digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if D == 0 {
            return write!(f, "{}", self.raw);
        }
        let scale = Self::scale();
        let integer_part = self.raw / scale;
        let frac_part = self.raw % scale;
        write!(f, "{}.{:0>width$}", integer_part, frac_part, width = D as usize)
    }
}

impl<const D: u32> fmt::Debug for FixedPoint<u128, D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FixedPoint<u128, {}>(raw={})", D, self.raw)
    }
}

impl<const D: u32> fmt::Display for FixedPoint<u128, D> {
    /// Formats as `integer.fractional` with exactly `DECIMALS` fractional digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if D == 0 {
            return write!(f, "{}", self.raw);
        }
        let scale = Self::scale();
        let integer_part = self.raw / scale;
        let frac_part = self.raw % scale;
        write!(f, "{}.{:0>width$}", integer_part, frac_part, width = D as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    type Fp6 = FixedPoint<u64, 6>;
    type Fp18 = FixedPoint<u128, 18>;

    #[test]
    fn display_u64_integer() {
        let v = Fp6::from_raw(3_000_000);
        assert_eq!(format!("{}", v), "3.000000");
    }

    #[test]
    fn display_u64_fractional() {
        let v = Fp6::from_raw(1_500_000);
        assert_eq!(format!("{}", v), "1.500000");
    }

    #[test]
    fn display_u64_zero() {
        assert_eq!(format!("{}", Fp6::ZERO), "0.000000");
    }

    #[test]
    fn display_u128() {
        let v = Fp18::from_raw(2_500_000_000_000_000_000);
        assert_eq!(format!("{}", v), "2.500000000000000000");
    }

    #[test]
    fn debug_u64() {
        let v = Fp6::from_raw(42);
        let s = format!("{:?}", v);
        assert!(s.contains("FixedPoint<u64, 6>"));
        assert!(s.contains("42"));
    }

    #[test]
    fn display_zero_decimals() {
        let v = FixedPoint::<u64, 0>::from_raw(42);
        assert_eq!(format!("{}", v), "42");
    }
}
