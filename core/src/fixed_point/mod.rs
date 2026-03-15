//! Fixed-point decimal arithmetic.
//!
//! `FixedPoint<T, DECIMALS>` provides deterministic decimal arithmetic
//! over integer backing types. All operations return `Option` on overflow.
//!
//! ```ignore
//! type Amount = FixedPoint<u128, 18>;
//! let a = Amount::from_integer(100);
//! let b = Amount::from_decimal(3, 14).unwrap(); // 3.14
//! let c = a.checked_mul(b).unwrap();            // 314.00
//! ```

mod ops;
mod fmt;
mod convert;

/// Fixed-point decimal number backed by integer type `T` with
/// `DECIMALS` fractional digits.
///
/// The raw value is `value * 10^DECIMALS`. For example,
/// `FixedPoint<u64, 6>` with raw value `1_000_000` represents `1.0`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FixedPoint<T, const DECIMALS: u32> {
    raw: T,
}

impl<T, const DECIMALS: u32> FixedPoint<T, DECIMALS> {
    /// Construct from the raw underlying integer.
    ///
    /// The caller is responsible for interpreting the scale correctly.
    pub const fn from_raw(raw: T) -> Self {
        Self { raw }
    }

    /// Access the raw underlying integer value.
    pub const fn raw(&self) -> &T {
        &self.raw
    }

    /// Consume self and return the raw value.
    pub fn into_raw(self) -> T {
        self.raw
    }
}

// --- u64 specialization ---

impl<const DECIMALS: u32> FixedPoint<u64, DECIMALS> {
    /// The zero value.
    pub const ZERO: Self = Self { raw: 0 };

    /// The value `1.0` (i.e., `10^DECIMALS`), or `None` at compile time
    /// if it overflows `u64`.
    pub const ONE: Self = Self { raw: Self::scale_u64() };

    /// The maximum representable value.
    pub const MAX: Self = Self { raw: u64::MAX };

    /// Compute `10^DECIMALS` as a `u64`.
    ///
    /// Panics at compile time if the scale overflows `u64`.
    const fn scale_u64() -> u64 {
        let mut result = 1u64;
        let mut i = 0;
        while i < DECIMALS {
            // This will panic at compile time if scale overflows u64.
            result = result * 10;
            i += 1;
        }
        result
    }

    /// The scale factor `10^DECIMALS`.
    pub const fn scale() -> u64 {
        Self::scale_u64()
    }
}

// --- u128 specialization ---

impl<const DECIMALS: u32> FixedPoint<u128, DECIMALS> {
    /// The zero value.
    pub const ZERO: Self = Self { raw: 0 };

    /// The value `1.0` (i.e., `10^DECIMALS`).
    pub const ONE: Self = Self { raw: Self::scale_u128() };

    /// The maximum representable value.
    pub const MAX: Self = Self { raw: u128::MAX };

    /// Compute `10^DECIMALS` as a `u128`.
    const fn scale_u128() -> u128 {
        let mut result = 1u128;
        let mut i = 0;
        while i < DECIMALS {
            result = result * 10;
            i += 1;
        }
        result
    }

    /// The scale factor `10^DECIMALS`.
    pub const fn scale() -> u128 {
        Self::scale_u128()
    }
}

impl<T: PartialOrd, const DECIMALS: u32> PartialOrd for FixedPoint<T, DECIMALS> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.raw.partial_cmp(&other.raw)
    }
}

impl<T: Ord, const DECIMALS: u32> Ord for FixedPoint<T, DECIMALS> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Fp6 = FixedPoint<u64, 6>;
    type Fp18 = FixedPoint<u128, 18>;

    #[test]
    fn scale_values() {
        assert_eq!(Fp6::scale(), 1_000_000);
        assert_eq!(Fp18::scale(), 1_000_000_000_000_000_000);
    }

    #[test]
    fn zero_and_one() {
        assert_eq!(Fp6::ZERO.raw(), &0);
        assert_eq!(Fp6::ONE.raw(), &1_000_000);
    }

    #[test]
    fn ordering() {
        let a = Fp6::from_raw(100);
        let b = Fp6::from_raw(200);
        assert!(a < b);
    }
}
