//! Arithmetic operations for `FixedPoint`.
//!
//! Provides checked, saturating, and wrapping variants for add, sub, mul, div.
//! All operations return `Option` or saturated/wrapped values — no panics.

use super::FixedPoint;

// =============================================================================
// u64 arithmetic
// =============================================================================

impl<const D: u32> FixedPoint<u64, D> {
    /// Checked addition. Returns `None` on overflow.
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.raw.checked_add(rhs.raw) {
            Some(r) => Some(Self { raw: r }),
            None => None,
        }
    }

    /// Checked subtraction. Returns `None` on underflow.
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.raw.checked_sub(rhs.raw) {
            Some(r) => Some(Self { raw: r }),
            None => None,
        }
    }

    /// Checked multiplication. Returns `None` on overflow.
    ///
    /// Uses u128 widening: `(a * b) / scale`.
    pub const fn checked_mul(self, rhs: Self) -> Option<Self> {
        let wide = (self.raw as u128).checked_mul(rhs.raw as u128);
        match wide {
            Some(w) => {
                let scale = Self::scale_u64() as u128;
                let result = w / scale;
                if result > u64::MAX as u128 {
                    None
                } else {
                    Some(Self { raw: result as u64 })
                }
            }
            None => None,
        }
    }

    /// Checked division. Returns `None` on division by zero or overflow.
    ///
    /// Uses u128 widening: `(a * scale) / b`.
    pub const fn checked_div(self, rhs: Self) -> Option<Self> {
        if rhs.raw == 0 {
            return None;
        }
        let scale = Self::scale_u64() as u128;
        let wide = (self.raw as u128).checked_mul(scale);
        match wide {
            Some(w) => {
                let result = w / (rhs.raw as u128);
                if result > u64::MAX as u128 {
                    None
                } else {
                    Some(Self { raw: result as u64 })
                }
            }
            None => None,
        }
    }

    /// Saturating addition. Clamps at `MAX` on overflow.
    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self { raw: self.raw.saturating_add(rhs.raw) }
    }

    /// Saturating subtraction. Clamps at zero on underflow.
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self { raw: self.raw.saturating_sub(rhs.raw) }
    }

    /// Saturating multiplication. Clamps at `MAX` on overflow.
    pub const fn saturating_mul(self, rhs: Self) -> Self {
        match self.checked_mul(rhs) {
            Some(v) => v,
            None => Self::MAX,
        }
    }

    /// Wrapping addition.
    pub const fn wrapping_add(self, rhs: Self) -> Self {
        Self { raw: self.raw.wrapping_add(rhs.raw) }
    }

    /// Wrapping subtraction.
    pub const fn wrapping_sub(self, rhs: Self) -> Self {
        Self { raw: self.raw.wrapping_sub(rhs.raw) }
    }

    /// Wrapping multiplication.
    ///
    /// Wraps the raw result of `(a * b) / scale`.
    pub const fn wrapping_mul(self, rhs: Self) -> Self {
        let wide = (self.raw as u128).wrapping_mul(rhs.raw as u128);
        let scale = Self::scale_u64() as u128;
        Self { raw: (wide / scale) as u64 }
    }
}

// =============================================================================
// u128 arithmetic — requires u256 emulation for checked_mul
// =============================================================================

/// 256-bit unsigned integer represented as two u128 halves (lo, hi).
#[derive(Clone, Copy)]
struct U256 {
    lo: u128,
    hi: u128,
}

impl U256 {
    /// Multiply two u128 values to produce a u256 result.
    ///
    /// Splits each operand into two u64 halves and performs schoolbook
    /// multiplication with carry propagation.
    const fn widening_mul(a: u128, b: u128) -> Self {
        let a_lo = a as u64 as u128;
        let a_hi = (a >> 64) as u64 as u128;
        let b_lo = b as u64 as u128;
        let b_hi = (b >> 64) as u64 as u128;

        let ll = a_lo * b_lo;
        let lh = a_lo * b_hi;
        let hl = a_hi * b_lo;
        let hh = a_hi * b_hi;

        // Accumulate cross terms
        let mid = lh + (ll >> 64);
        let mid_lo = mid as u64 as u128;
        let mid_hi = mid >> 64;

        let mid2 = mid_lo + hl;
        let mid2_carry = mid2 >> 64;

        let lo = ((mid2 as u64 as u128) << 64) | (ll as u64 as u128);
        let hi = hh + mid_hi + mid2_carry;

        Self { lo, hi }
    }

    /// Divide a u256 by a u128 divisor, returning the u128 quotient.
    ///
    /// Returns `None` if the quotient overflows u128 or divisor is zero.
    const fn checked_div_u128(self, divisor: u128) -> Option<u128> {
        if divisor == 0 {
            return None;
        }

        // If hi is zero, simple u128 division.
        if self.hi == 0 {
            return Some(self.lo / divisor);
        }

        // Check if quotient fits in u128: hi < divisor is a necessary
        // condition for the result to fit.
        if self.hi >= divisor {
            return None;
        }

        // Long division: divide (hi:lo) by divisor.
        // Since hi < divisor, the quotient fits in u128.
        //
        // We split this into two 64-bit division steps.
        let mut remainder = self.hi;

        // Process high 64 bits of lo
        let lo_hi = self.lo >> 64;
        let dividend_hi = (remainder << 64) | lo_hi;
        let q_hi = dividend_hi / divisor;
        remainder = dividend_hi % divisor;

        // Process low 64 bits of lo
        let lo_lo = self.lo as u64 as u128;
        let dividend_lo = (remainder << 64) | lo_lo;
        let q_lo = dividend_lo / divisor;

        // Combine quotient halves
        // q_hi should fit in 64 bits since hi < divisor
        Some((q_hi << 64) | q_lo)
    }
}

impl<const D: u32> FixedPoint<u128, D> {
    /// Checked addition. Returns `None` on overflow.
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.raw.checked_add(rhs.raw) {
            Some(r) => Some(Self { raw: r }),
            None => None,
        }
    }

    /// Checked subtraction. Returns `None` on underflow.
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.raw.checked_sub(rhs.raw) {
            Some(r) => Some(Self { raw: r }),
            None => None,
        }
    }

    /// Checked multiplication. Returns `None` on overflow.
    ///
    /// Uses u256 widening multiplication: `(a * b) / scale`.
    /// The u256 is split into high/low u128 halves for the multiply,
    /// then divided by the scale factor.
    pub const fn checked_mul(self, rhs: Self) -> Option<Self> {
        let wide = U256::widening_mul(self.raw, rhs.raw);
        let scale = Self::scale_u128();
        match wide.checked_div_u128(scale) {
            Some(result) => Some(Self { raw: result }),
            None => None,
        }
    }

    /// Checked division. Returns `None` on division by zero or overflow.
    ///
    /// Uses u256 widening: `(a * scale) / b`.
    pub const fn checked_div(self, rhs: Self) -> Option<Self> {
        if rhs.raw == 0 {
            return None;
        }
        let scale = Self::scale_u128();
        let wide = U256::widening_mul(self.raw, scale);
        match wide.checked_div_u128(rhs.raw) {
            Some(result) => Some(Self { raw: result }),
            None => None,
        }
    }

    /// Saturating addition.
    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self { raw: self.raw.saturating_add(rhs.raw) }
    }

    /// Saturating subtraction.
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self { raw: self.raw.saturating_sub(rhs.raw) }
    }

    /// Saturating multiplication.
    pub const fn saturating_mul(self, rhs: Self) -> Self {
        match self.checked_mul(rhs) {
            Some(v) => v,
            None => Self::MAX,
        }
    }

    /// Wrapping addition.
    pub const fn wrapping_add(self, rhs: Self) -> Self {
        Self { raw: self.raw.wrapping_add(rhs.raw) }
    }

    /// Wrapping subtraction.
    pub const fn wrapping_sub(self, rhs: Self) -> Self {
        Self { raw: self.raw.wrapping_sub(rhs.raw) }
    }

    /// Wrapping multiplication.
    pub const fn wrapping_mul(self, rhs: Self) -> Self {
        let wide = U256::widening_mul(self.raw, rhs.raw);
        let scale = Self::scale_u128();
        // On overflow, use modular result
        match wide.checked_div_u128(scale) {
            Some(r) => Self { raw: r },
            None => {
                // Overflow — wrap by taking the low bits
                let lo_result = wide.lo.wrapping_div(scale);
                Self { raw: lo_result }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Fp6 = FixedPoint<u64, 6>;
    type Fp18 = FixedPoint<u128, 18>;

    // --- u64 tests ---

    #[test]
    fn u64_checked_add() {
        let a = Fp6::from_raw(1_000_000); // 1.0
        let b = Fp6::from_raw(2_000_000); // 2.0
        let c = a.checked_add(b).unwrap();
        assert_eq!(*c.raw(), 3_000_000);
    }

    #[test]
    fn u64_checked_add_overflow() {
        let a = Fp6::MAX;
        let b = Fp6::from_raw(1);
        assert!(a.checked_add(b).is_none());
    }

    #[test]
    fn u64_checked_sub() {
        let a = Fp6::from_raw(3_000_000);
        let b = Fp6::from_raw(1_000_000);
        let c = a.checked_sub(b).unwrap();
        assert_eq!(*c.raw(), 2_000_000);
    }

    #[test]
    fn u64_checked_sub_underflow() {
        let a = Fp6::ZERO;
        let b = Fp6::from_raw(1);
        assert!(a.checked_sub(b).is_none());
    }

    #[test]
    fn u64_checked_mul() {
        let a = Fp6::from_raw(2_000_000); // 2.0
        let b = Fp6::from_raw(3_000_000); // 3.0
        let c = a.checked_mul(b).unwrap();
        assert_eq!(*c.raw(), 6_000_000); // 6.0
    }

    #[test]
    fn u64_checked_mul_fractional() {
        let a = Fp6::from_raw(1_500_000); // 1.5
        let b = Fp6::from_raw(2_000_000); // 2.0
        let c = a.checked_mul(b).unwrap();
        assert_eq!(*c.raw(), 3_000_000); // 3.0
    }

    #[test]
    fn u64_checked_div() {
        let a = Fp6::from_raw(6_000_000); // 6.0
        let b = Fp6::from_raw(2_000_000); // 2.0
        let c = a.checked_div(b).unwrap();
        assert_eq!(*c.raw(), 3_000_000); // 3.0
    }

    #[test]
    fn u64_checked_div_by_zero() {
        let a = Fp6::from_raw(1_000_000);
        assert!(a.checked_div(Fp6::ZERO).is_none());
    }

    #[test]
    fn u64_saturating_add() {
        let a = Fp6::MAX;
        let b = Fp6::from_raw(1);
        assert_eq!(a.saturating_add(b), Fp6::MAX);
    }

    #[test]
    fn u64_saturating_sub() {
        let a = Fp6::ZERO;
        let b = Fp6::from_raw(1);
        assert_eq!(a.saturating_sub(b), Fp6::ZERO);
    }

    #[test]
    fn u64_wrapping_add() {
        let a = Fp6::MAX;
        let b = Fp6::from_raw(1);
        assert_eq!(a.wrapping_add(b), Fp6::ZERO);
    }

    #[test]
    fn u64_mul_by_zero() {
        let a = Fp6::from_raw(1_000_000);
        let c = a.checked_mul(Fp6::ZERO).unwrap();
        assert_eq!(*c.raw(), 0);
    }

    #[test]
    fn u64_mul_by_one() {
        let a = Fp6::from_raw(12_345_678);
        let c = a.checked_mul(Fp6::ONE).unwrap();
        assert_eq!(*c.raw(), 12_345_678);
    }

    // --- u128 tests ---

    #[test]
    fn u128_checked_add() {
        let a = Fp18::from_raw(1_000_000_000_000_000_000); // 1.0
        let b = Fp18::from_raw(2_000_000_000_000_000_000); // 2.0
        let c = a.checked_add(b).unwrap();
        assert_eq!(*c.raw(), 3_000_000_000_000_000_000);
    }

    #[test]
    fn u128_checked_mul() {
        let a = Fp18::from_raw(2_000_000_000_000_000_000); // 2.0
        let b = Fp18::from_raw(3_000_000_000_000_000_000); // 3.0
        let c = a.checked_mul(b).unwrap();
        assert_eq!(*c.raw(), 6_000_000_000_000_000_000); // 6.0
    }

    #[test]
    fn u128_checked_mul_fractional() {
        let scale = Fp18::scale();
        let a = Fp18::from_raw(scale / 2); // 0.5
        let b = Fp18::from_raw(scale / 4); // 0.25
        let c = a.checked_mul(b).unwrap();
        assert_eq!(*c.raw(), scale / 8); // 0.125
    }

    #[test]
    fn u128_checked_div() {
        let a = Fp18::from_raw(6_000_000_000_000_000_000); // 6.0
        let b = Fp18::from_raw(2_000_000_000_000_000_000); // 2.0
        let c = a.checked_div(b).unwrap();
        assert_eq!(*c.raw(), 3_000_000_000_000_000_000); // 3.0
    }

    #[test]
    fn u128_checked_div_by_zero() {
        let a = Fp18::from_raw(1_000_000_000_000_000_000);
        assert!(a.checked_div(Fp18::ZERO).is_none());
    }

    #[test]
    fn u128_mul_by_one() {
        let a = Fp18::from_raw(123_456_789_000_000_000);
        let c = a.checked_mul(Fp18::ONE).unwrap();
        assert_eq!(*c.raw(), 123_456_789_000_000_000);
    }

    #[test]
    fn u128_large_mul() {
        // Test that widening multiplication handles large values
        let a = Fp18::from_raw(100_000_000_000_000_000_000u128); // 100.0
        let b = Fp18::from_raw(200_000_000_000_000_000_000u128); // 200.0
        let c = a.checked_mul(b).unwrap();
        assert_eq!(*c.raw(), 20_000_000_000_000_000_000_000u128); // 20000.0
    }

    #[test]
    fn u256_widening_mul_basic() {
        let r = U256::widening_mul(10, 20);
        assert_eq!(r.lo, 200);
        assert_eq!(r.hi, 0);
    }

    #[test]
    fn u256_widening_mul_large() {
        // (2^127) * 2 = 2^128 — should overflow into hi
        let a: u128 = 1 << 127;
        let r = U256::widening_mul(a, 2);
        assert_eq!(r.lo, 0);
        assert_eq!(r.hi, 1);
    }

    #[test]
    fn u256_div_basic() {
        let v = U256 { lo: 200, hi: 0 };
        assert_eq!(v.checked_div_u128(10), Some(20));
    }

    #[test]
    fn u256_div_with_hi() {
        // 2^128 / 2 = 2^127
        let v = U256 { lo: 0, hi: 1 };
        assert_eq!(v.checked_div_u128(2), Some(1u128 << 127));
    }

    #[test]
    fn u256_div_by_zero() {
        let v = U256 { lo: 100, hi: 0 };
        assert!(v.checked_div_u128(0).is_none());
    }
}
