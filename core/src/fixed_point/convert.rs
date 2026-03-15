//! Conversion functions for `FixedPoint`.
//!
//! Provides `from_integer`, `from_decimal`, and `From` trait implementations.

use super::FixedPoint;

impl<const D: u32> FixedPoint<u64, D> {
    /// Create from a whole integer value.
    ///
    /// Returns `None` if `value * 10^DECIMALS` overflows `u64`.
    pub const fn from_integer(value: u64) -> Option<Self> {
        let scale = Self::scale_u64();
        match value.checked_mul(scale) {
            Some(raw) => Some(Self { raw }),
            None => None,
        }
    }

    /// Create from integer and fractional parts.
    ///
    /// `from_decimal(3, 14)` with `DECIMALS=6` produces `3.140000`.
    /// The `frac` value is interpreted as the high-order fractional digits.
    ///
    /// Returns `None` on overflow or if `frac >= 10^DECIMALS`.
    pub const fn from_decimal(integer: u64, frac: u64) -> Option<Self> {
        let scale = Self::scale_u64();
        if frac >= scale {
            return None;
        }

        // Count digits in frac to determine padding
        let frac_digits = count_digits_u64(frac);
        if frac_digits > D {
            return None;
        }

        // Pad frac to the left: e.g., frac=14, D=6 -> 140000
        let pad = D - frac_digits;
        let mut padded_frac = frac;
        let mut i = 0;
        while i < pad {
            padded_frac = match padded_frac.checked_mul(10) {
                Some(v) => v,
                None => return None,
            };
            i += 1;
        }

        match integer.checked_mul(scale) {
            Some(int_raw) => match int_raw.checked_add(padded_frac) {
                Some(raw) => Some(Self { raw }),
                None => None,
            },
            None => None,
        }
    }

    /// Extract the integer part (truncated toward zero).
    pub const fn to_integer(self) -> u64 {
        self.raw / Self::scale_u64()
    }

    /// Extract the fractional part as raw sub-units.
    pub const fn frac_raw(self) -> u64 {
        self.raw % Self::scale_u64()
    }
}

impl<const D: u32> FixedPoint<u128, D> {
    /// Create from a whole integer value.
    ///
    /// Returns `None` if `value * 10^DECIMALS` overflows `u128`.
    pub const fn from_integer(value: u128) -> Option<Self> {
        let scale = Self::scale_u128();
        match value.checked_mul(scale) {
            Some(raw) => Some(Self { raw }),
            None => None,
        }
    }

    /// Create from integer and fractional parts.
    ///
    /// Returns `None` on overflow or if `frac >= 10^DECIMALS`.
    pub const fn from_decimal(integer: u128, frac: u128) -> Option<Self> {
        let scale = Self::scale_u128();
        if frac >= scale {
            return None;
        }

        let frac_digits = count_digits_u128(frac);
        if frac_digits > D {
            return None;
        }

        let pad = D - frac_digits;
        let mut padded_frac = frac;
        let mut i = 0;
        while i < pad {
            padded_frac = match padded_frac.checked_mul(10) {
                Some(v) => v,
                None => return None,
            };
            i += 1;
        }

        match integer.checked_mul(scale) {
            Some(int_raw) => match int_raw.checked_add(padded_frac) {
                Some(raw) => Some(Self { raw }),
                None => None,
            },
            None => None,
        }
    }

    /// Extract the integer part (truncated toward zero).
    pub const fn to_integer(self) -> u128 {
        self.raw / Self::scale_u128()
    }

    /// Extract the fractional part as raw sub-units.
    pub const fn frac_raw(self) -> u128 {
        self.raw % Self::scale_u128()
    }
}

/// Count the number of decimal digits in a u64 value.
/// Returns 0 for input 0 (treated as zero-digit number for padding purposes).
const fn count_digits_u64(mut n: u64) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut count = 0;
    while n > 0 {
        count += 1;
        n /= 10;
    }
    count
}

/// Count the number of decimal digits in a u128 value.
const fn count_digits_u128(mut n: u128) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut count = 0;
    while n > 0 {
        count += 1;
        n /= 10;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    type Fp6 = FixedPoint<u64, 6>;
    type Fp18 = FixedPoint<u128, 18>;

    #[test]
    fn from_integer_u64() {
        let v = Fp6::from_integer(42).unwrap();
        assert_eq!(*v.raw(), 42_000_000);
        assert_eq!(v.to_integer(), 42);
    }

    #[test]
    fn from_integer_overflow_u64() {
        // u64::MAX / 10^6 is ~18_446_744_073_709, anything above overflows
        assert!(Fp6::from_integer(u64::MAX).is_none());
    }

    #[test]
    fn from_decimal_u64() {
        let v = Fp6::from_decimal(3, 14).unwrap();
        assert_eq!(*v.raw(), 3_140_000);
    }

    #[test]
    fn from_decimal_zero_frac_u64() {
        let v = Fp6::from_decimal(5, 0).unwrap();
        assert_eq!(*v.raw(), 5_000_000);
    }

    #[test]
    fn from_decimal_frac_too_large_u64() {
        assert!(Fp6::from_decimal(1, 1_000_000).is_none());
    }

    #[test]
    fn from_integer_u128() {
        let v = Fp18::from_integer(100).unwrap();
        assert_eq!(*v.raw(), 100_000_000_000_000_000_000u128);
        assert_eq!(v.to_integer(), 100);
    }

    #[test]
    fn from_decimal_u128() {
        let v = Fp18::from_decimal(3, 14).unwrap();
        assert_eq!(v.to_integer(), 3);
        // frac=14, padded to 18 digits = 14 * 10^16 = 140_000_000_000_000_000
        assert_eq!(v.frac_raw(), 140_000_000_000_000_000);
    }

    #[test]
    fn to_integer_truncates() {
        let v = Fp6::from_raw(7_999_999); // 7.999999
        assert_eq!(v.to_integer(), 7);
    }

    #[test]
    fn frac_raw_correct() {
        let v = Fp6::from_raw(3_140_000);
        assert_eq!(v.frac_raw(), 140_000);
    }

    #[test]
    fn count_digits() {
        assert_eq!(count_digits_u64(0), 0);
        assert_eq!(count_digits_u64(1), 1);
        assert_eq!(count_digits_u64(9), 1);
        assert_eq!(count_digits_u64(10), 2);
        assert_eq!(count_digits_u64(999), 3);
        assert_eq!(count_digits_u64(1000), 4);
    }
}
