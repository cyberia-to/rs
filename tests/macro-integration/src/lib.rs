//! Integration tests for rs-lang proc-macros.
//!
//! This crate verifies that generated code from all proc-macros compiles
//! and behaves correctly when used against the rs-lang runtime types.

use core::time::Duration;
use rs_lang::prelude::*;
use rs_lang::{cell, step, Addressed, deterministic};

// ---------------------------------------------------------------------------
// #[derive(Addressed)] — canonical serialization + particle
// ---------------------------------------------------------------------------

#[derive(Addressed)]
struct SimpleRecord {
    id: u32,
    value: u64,
    flag: bool,
}

#[derive(Addressed)]
struct WithOption {
    data: u32,
    extra: Option<u64>,
}

#[derive(Addressed)]
struct WithArray {
    hash: [u8; 32],
}

#[derive(Addressed)]
enum Status {
    Active,
    Inactive,
    Suspended(u32),
}

// ---------------------------------------------------------------------------
// #[step] — struct reset
// ---------------------------------------------------------------------------

#[step]
struct StepCounters {
    count: u32,
    total: u64,
    active: bool,
    maybe: Option<u32>,
}

// ---------------------------------------------------------------------------
// #[deterministic] — token-level checks (should compile fine)
// ---------------------------------------------------------------------------

#[deterministic]
fn add_checked(a: u32, b: u32) -> Option<u32> {
    a.checked_add(b)
}

#[deterministic]
fn multiply_fixed(a: u64, b: u64) -> u64 {
    a.wrapping_mul(b)
}

// ---------------------------------------------------------------------------
// cell! — full cell declaration
// ---------------------------------------------------------------------------

cell! {
    name: Counter,
    version: 1,
    budget: Duration::from_millis(100),
    heartbeat: Duration::from_secs(1),

    state {
        value: u64,
        limit: u32,
    }

    step_state {
        increments: u32,
    }

    pub fn get(&self) -> u64 {
        self.state.value
    }

    pub fn increment(&mut self) -> Result<u64> {
        if self.step_state.increments >= self.state.limit {
            return Err(Error::LimitReached);
        }
        self.state.value = self.state.value.wrapping_add(1);
        self.step_state.increments += 1;
        Ok(self.state.value)
    }

    fn internal_check(&self) -> bool {
        self.state.value < u64::MAX
    }
}

// ---------------------------------------------------------------------------
// cell! — minimal cell (no step_state, no migrate)
// ---------------------------------------------------------------------------

cell! {
    name: Minimal,
    version: 1,
    budget: Duration::from_millis(50),
    heartbeat: Duration::from_secs(5),

    state {
        alive: bool,
    }

    pub fn is_alive(&self) -> bool {
        self.state.alive
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rs_lang::{Cell, CellMetadata, StepReset};

    // --- Addressed ---

    #[test]
    fn addressed_serialize_deterministic() {
        let rec = SimpleRecord { id: 42, value: 100, flag: true };
        let p1 = rec.particle();
        let p2 = rec.particle();
        assert_eq!(p1, p2, "same value must produce same particle");
    }

    #[test]
    fn addressed_different_values_different_particles() {
        let a = SimpleRecord { id: 1, value: 0, flag: false };
        let b = SimpleRecord { id: 2, value: 0, flag: false };
        assert_ne!(a.particle(), b.particle());
    }

    #[test]
    fn addressed_option_some_vs_none() {
        let some = WithOption { data: 1, extra: Some(99) };
        let none = WithOption { data: 1, extra: None };
        assert_ne!(some.particle(), none.particle());
    }

    #[test]
    fn addressed_array_field() {
        let rec = WithArray { hash: [0xAA; 32] };
        let p = rec.particle();
        assert_ne!(p, Particle::ZERO);
    }

    #[test]
    fn addressed_enum_variants() {
        let a = Status::Active;
        let b = Status::Inactive;
        let c = Status::Suspended(42);
        assert_ne!(a.particle(), b.particle());
        assert_ne!(b.particle(), c.particle());
    }

    // --- Step ---

    #[test]
    fn step_reset_zeros_fields() {
        let mut counters = StepCounters {
            count: 42,
            total: 999,
            active: true,
            maybe: Some(7),
        };
        counters.reset();
        assert_eq!(counters.count, 0);
        assert_eq!(counters.total, 0);
        assert!(!counters.active);
        assert!(counters.maybe.is_none());
    }

    // --- Deterministic ---

    #[test]
    fn deterministic_fn_works() {
        assert_eq!(add_checked(1, 2), Some(3));
        assert_eq!(add_checked(u32::MAX, 1), None);
        assert_eq!(multiply_fixed(3, 7), 21);
    }

    // --- Cell: Counter ---

    #[test]
    fn cell_trait_constants() {
        assert_eq!(Counter::NAME, "Counter");
        assert_eq!(Counter::VERSION, 1);
        assert_eq!(Counter::BUDGET, Duration::from_millis(100));
        assert_eq!(Counter::HEARTBEAT, Duration::from_secs(1));
    }

    #[test]
    fn cell_constructor() {
        let cell = Counter::new();
        assert_eq!(cell.current_step(), 0);
        assert_eq!(cell.get(), 0);
    }

    #[test]
    fn cell_increment_and_limit() {
        let mut cell = Counter::new();
        // limit defaults to 0, so first increment should fail
        // Actually Default for u32 is 0, so limit is 0
        let result = cell.increment();
        assert!(result.is_err());
    }

    #[test]
    fn cell_health_check() {
        let cell = Counter::new();
        assert_eq!(cell.health_check(), HealthStatus::Healthy);
    }

    #[test]
    fn cell_step_reset() {
        let mut cell = Counter::new();
        // step_state.increments starts at 0 (default)
        cell.reset_step_state();
        // Should still be 0 after reset
        assert_eq!(cell.get(), 0);
    }

    #[test]
    fn cell_metadata_interface() {
        let sigs = Counter::interface();
        let names: Vec<&str> = sigs.iter().map(|s| s.name).collect();
        assert!(names.contains(&"get"), "missing 'get' in interface");
        assert!(names.contains(&"increment"), "missing 'increment' in interface");
        // internal_check is private — should not appear
        assert!(!names.contains(&"internal_check"), "private fn leaked to interface");
    }

    #[test]
    fn cell_error_type_exists() {
        // CounterError should exist and have LimitReached variant
        let _err: CounterError = CounterError::LimitReached;
        // Debug should be derived
        let _ = format!("{:?}", _err);
    }

    // --- Cell: Minimal ---

    #[test]
    fn minimal_cell() {
        let cell = Minimal::new();
        assert_eq!(Minimal::NAME, "Minimal");
        assert_eq!(Minimal::VERSION, 1);
        assert!(!cell.is_alive()); // bool defaults to false
    }

    #[test]
    fn minimal_cell_metadata() {
        let sigs = Minimal::interface();
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "is_alive");
    }
}
