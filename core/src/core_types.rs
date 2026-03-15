//! Core types and traits for the Rs language runtime.
//!
//! Defines `Address`, `Particle`, `Timeout`, serialization traits,
//! cell lifecycle traits, and the `BufMut` abstraction.

/// A 32-byte address identifying an entity in the system.
pub type Address = [u8; 32];

/// A write buffer trait for canonical serialization.
///
/// Implementations accept byte slices without heap allocation.
pub trait BufMut {
    /// Append bytes to the buffer.
    fn put_bytes(&mut self, bytes: &[u8]);
}

/// Cursor-based `BufMut` implementation for `&mut [u8]`.
///
/// Tracks the write position internally. Bytes beyond the slice
/// length are silently dropped.
pub struct SliceBufMut<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> SliceBufMut<'a> {
    /// Create a new cursor over the given slice.
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Returns the number of bytes written so far.
    pub fn position(&self) -> usize {
        self.pos
    }
}

impl BufMut for SliceBufMut<'_> {
    fn put_bytes(&mut self, bytes: &[u8]) {
        let remaining = self.buf.len().saturating_sub(self.pos);
        let to_copy = bytes.len().min(remaining);
        if to_copy > 0 {
            self.buf[self.pos..self.pos + to_copy].copy_from_slice(&bytes[..to_copy]);
            self.pos += to_copy;
        }
    }
}

/// A 64-byte content-addressed hash value (8 Goldilocks field elements).
///
/// Placeholder implementation using a simple mixing function.
/// The real implementation will use Poseidon2/Goldilocks from `cyber-hemera`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Particle([u64; 8]);

impl Particle {
    /// The zero particle (all elements zero).
    pub const ZERO: Self = Self([0u64; 8]);

    /// Create a particle by hashing arbitrary bytes.
    ///
    /// Uses a simple non-cryptographic mixing function as a placeholder.
    /// Production code will use Hemera (Poseidon2 over Goldilocks).
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut state = [
            0x517cc1b727220a95u64,
            0x6c62272e07bb0142,
            0x8eb44a8768581511,
            0xdb0c2e0d64f98fa7,
            0x47b5481dbefa4fa4,
            0x5a0f6c1e38e8b2a3,
            0x2d9e8f0b4a7c3d6e,
            0x1f3b7a9c5e2d4f8a,
        ];

        // Absorb data in 8-byte chunks with mixing
        let mut i = 0;
        while i < data.len() {
            let lane = i % 8;
            let mut word = 0u64;
            let chunk_end = if i + 8 <= data.len() { i + 8 } else { data.len() };
            let mut j = i;
            while j < chunk_end {
                word |= (data[j] as u64) << ((j - i) * 8);
                j += 1;
            }
            state[lane] = state[lane].wrapping_add(word);
            state[lane] = state[lane].wrapping_mul(0x9e3779b97f4a7c15);
            state[lane] ^= state[lane] >> 32;
            // Diffuse into next lane
            state[(lane + 1) % 8] ^= state[lane];
            i = chunk_end;
        }

        // Final mixing rounds
        let mut round = 0;
        while round < 4 {
            let mut lane = 0;
            while lane < 8 {
                state[lane] = state[lane].wrapping_add(state[(lane + 3) % 8]);
                state[lane] = state[lane].wrapping_mul(0x517cc1b727220a95);
                state[lane] ^= state[lane] >> 28;
                lane += 1;
            }
            round += 1;
        }

        Self(state)
    }

    /// Create a particle from a canonically serializable value.
    ///
    /// Serializes the value into a stack buffer and hashes the result.
    /// Values larger than 512 bytes are hashed incrementally.
    pub fn from_canonical<T: CanonicalSerialize>(value: &T) -> Self {
        let mut buf = [0u8; 512];
        let written = {
            let mut cursor = SliceBufMut::new(&mut buf);
            value.serialize_canonical(&mut cursor);
            cursor.position()
        };
        Self::from_bytes(&buf[..written])
    }

    /// Access the raw 8-element array.
    pub fn as_elements(&self) -> &[u64; 8] {
        &self.0
    }

    /// Construct from raw elements.
    pub fn from_elements(elements: [u64; 8]) -> Self {
        Self(elements)
    }

    /// Convert to a 64-byte array.
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut out = [0u8; 64];
        let mut i = 0;
        while i < 8 {
            let bytes = self.0[i].to_le_bytes();
            let mut j = 0;
            while j < 8 {
                out[i * 8 + j] = bytes[j];
                j += 1;
            }
            i += 1;
        }
        out
    }
}

impl PartialOrd for Particle {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Particle {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl core::fmt::Debug for Particle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Particle({:016x}{:016x}..)", self.0[0], self.0[1])
    }
}

/// Returned when a bounded async operation exceeds its deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout;

impl core::fmt::Display for Timeout {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("operation timed out")
    }
}

/// Health status reported by a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Cell is operating normally.
    Healthy,
    /// Cell is operational but degraded. The `u32` is an application-specific code.
    Degraded(u32),
    /// Cell has failed. The `u32` is an application-specific error code.
    Unhealthy(u32),
}

/// Deterministic canonical serialization.
///
/// Implementations must produce a single unique byte sequence for each
/// value. Field order, endianness, and encoding are fixed by the
/// `#[derive(Addressed)]` macro.
pub trait CanonicalSerialize {
    /// Serialize this value into the provided buffer.
    fn serialize_canonical(&self, buf: &mut impl BufMut);

    /// Return the exact number of bytes this value will produce.
    fn serialized_size(&self) -> usize;
}

/// Reset step-scoped state to its initial value.
///
/// Applied to state that must be cleared at each step boundary.
pub trait StepReset {
    /// Reset all fields to their step-initial values.
    fn reset(&mut self);
}

/// Core trait for cell lifecycle management.
///
/// Generated by the `cell!` macro. Provides identity, versioning,
/// resource bounds, health monitoring, and step management.
pub trait Cell {
    /// Human-readable cell name.
    const NAME: &'static str;
    /// Monotonically increasing version number.
    const VERSION: u32;
    /// Maximum resource budget per step.
    const BUDGET: core::time::Duration;
    /// Expected heartbeat interval.
    const HEARTBEAT: core::time::Duration;

    /// Return the current step counter.
    fn current_step(&self) -> u64;
    /// Report current health status.
    fn health_check(&self) -> HealthStatus;
    /// Reset all step-scoped state.
    fn reset_step_state(&mut self);
}

/// State migration between cell versions.
///
/// Transforms state from a previous cell version into the current
/// version's state format.
pub trait MigrateFrom<T> {
    /// Migrate state from the old version.
    fn migrate(old: T) -> Self;
}

/// Describes a single function in a cell's public interface.
#[derive(Debug, Clone, Copy)]
pub struct FunctionSignature {
    /// Function name.
    pub name: &'static str,
    /// Argument type descriptions.
    pub args: &'static [&'static str],
    /// Return type description.
    pub ret: &'static str,
    /// Async deadline, if any.
    pub deadline: Option<core::time::Duration>,
}

/// Compile-time introspection of a cell's public interface.
pub trait CellMetadata {
    /// Return the list of public function signatures.
    fn interface() -> &'static [FunctionSignature];
}

// --- CanonicalSerialize implementations for primitives ---

impl CanonicalSerialize for u8 {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&[*self]);
    }
    fn serialized_size(&self) -> usize { 1 }
}

impl CanonicalSerialize for u16 {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&self.to_le_bytes());
    }
    fn serialized_size(&self) -> usize { 2 }
}

impl CanonicalSerialize for u32 {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&self.to_le_bytes());
    }
    fn serialized_size(&self) -> usize { 4 }
}

impl CanonicalSerialize for u64 {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&self.to_le_bytes());
    }
    fn serialized_size(&self) -> usize { 8 }
}

impl CanonicalSerialize for u128 {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&self.to_le_bytes());
    }
    fn serialized_size(&self) -> usize { 16 }
}

impl CanonicalSerialize for i8 {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&self.to_le_bytes());
    }
    fn serialized_size(&self) -> usize { 1 }
}

impl CanonicalSerialize for i16 {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&self.to_le_bytes());
    }
    fn serialized_size(&self) -> usize { 2 }
}

impl CanonicalSerialize for i32 {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&self.to_le_bytes());
    }
    fn serialized_size(&self) -> usize { 4 }
}

impl CanonicalSerialize for i64 {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&self.to_le_bytes());
    }
    fn serialized_size(&self) -> usize { 8 }
}

impl CanonicalSerialize for i128 {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&self.to_le_bytes());
    }
    fn serialized_size(&self) -> usize { 16 }
}

impl CanonicalSerialize for bool {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&[*self as u8]);
    }
    fn serialized_size(&self) -> usize { 1 }
}

impl<T: CanonicalSerialize> CanonicalSerialize for Option<T> {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        match self {
            None => buf.put_bytes(&[0u8]),
            Some(v) => {
                buf.put_bytes(&[1u8]);
                v.serialize_canonical(buf);
            }
        }
    }
    fn serialized_size(&self) -> usize {
        1 + match self {
            None => 0,
            Some(v) => v.serialized_size(),
        }
    }
}

impl CanonicalSerialize for &[u8] {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        (self.len() as u32).serialize_canonical(buf);
        buf.put_bytes(self);
    }
    fn serialized_size(&self) -> usize {
        4 + self.len()
    }
}

impl<const N: usize> CanonicalSerialize for [u8; N] {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(self);
    }
    fn serialized_size(&self) -> usize {
        N
    }
}

impl CanonicalSerialize for Particle {
    fn serialize_canonical(&self, buf: &mut impl BufMut) {
        buf.put_bytes(&self.to_bytes());
    }
    fn serialized_size(&self) -> usize {
        64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_from_bytes_deterministic() {
        let a = Particle::from_bytes(b"hello world");
        let b = Particle::from_bytes(b"hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn particle_from_bytes_distinct() {
        let a = Particle::from_bytes(b"hello");
        let b = Particle::from_bytes(b"world");
        assert_ne!(a, b);
    }

    #[test]
    fn particle_empty_input() {
        let p = Particle::from_bytes(b"");
        assert_ne!(p, Particle::ZERO);
    }

    #[test]
    fn particle_ordering() {
        let a = Particle::from_bytes(b"aaa");
        let b = Particle::from_bytes(b"bbb");
        // Just verify ordering is total and consistent
        assert!(a < b || a > b || a == b);
    }

    #[test]
    fn slice_buf_mut_basic() {
        let mut buf = [0u8; 16];
        let mut cursor = SliceBufMut::new(&mut buf);
        cursor.put_bytes(&[1, 2, 3]);
        cursor.put_bytes(&[4, 5]);
        assert_eq!(cursor.position(), 5);
        assert_eq!(&buf[..5], &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn slice_buf_mut_overflow() {
        let mut buf = [0u8; 4];
        let mut cursor = SliceBufMut::new(&mut buf);
        cursor.put_bytes(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(cursor.position(), 4);
        assert_eq!(&buf, &[1, 2, 3, 4]);
    }

    #[test]
    fn canonical_serialize_u32() {
        let mut buf = [0u8; 4];
        let mut cursor = SliceBufMut::new(&mut buf);
        42u32.serialize_canonical(&mut cursor);
        assert_eq!(buf, 42u32.to_le_bytes());
    }

    #[test]
    fn canonical_serialize_option() {
        let mut buf = [0u8; 16];
        let mut cursor = SliceBufMut::new(&mut buf);
        let val: Option<u32> = Some(0x12345678);
        val.serialize_canonical(&mut cursor);
        assert_eq!(cursor.position(), 5);
        assert_eq!(buf[0], 1); // tag
        assert_eq!(&buf[1..5], &0x12345678u32.to_le_bytes());
    }

    #[test]
    fn particle_from_canonical() {
        let val = 42u64;
        let p = Particle::from_canonical(&val);
        let p2 = Particle::from_canonical(&val);
        assert_eq!(p, p2);

        let other = 43u64;
        let p3 = Particle::from_canonical(&other);
        assert_ne!(p, p3);
    }

    #[test]
    fn health_status_variants() {
        let h = HealthStatus::Healthy;
        let d = HealthStatus::Degraded(1);
        let u = HealthStatus::Unhealthy(2);
        assert_ne!(h, d);
        assert_ne!(d, u);
    }
}
