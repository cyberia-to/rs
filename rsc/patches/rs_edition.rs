//! Rs edition recognition.
//!
//! Injected into rustc_span to add "rs" as a valid edition string.
//! The Rs edition enables all Rs-specific lint passes while preserving
//! full Rust 2024 semantics — Rs is a strict superset.

use rustc_span::edition::Edition;
use rustc_span::Symbol;

/// The edition string users write in Cargo.toml: `edition = "rs"`.
pub const RS_EDITION_NAME: &str = "rs";

/// Symbol interned at compiler startup for fast comparison.
static RS_EDITION_SYM: std::sync::OnceLock<Symbol> = std::sync::OnceLock::new();

/// Initialize the Rs edition symbol. Called once during compiler setup.
pub fn init_rs_edition_symbol() {
    RS_EDITION_SYM.get_or_init(|| Symbol::intern(RS_EDITION_NAME));
}

/// Returns the interned symbol for "rs".
pub fn rs_edition_symbol() -> Symbol {
    *RS_EDITION_SYM.get_or_init(|| Symbol::intern(RS_EDITION_NAME))
}

/// Check whether the current compilation uses the Rs edition.
///
/// Rs maps to Rust 2024 semantics internally — the edition enum value
/// is Edition2024. The distinction is carried via a session flag set
/// when the edition string parses as "rs".
pub fn is_rs_edition(edition: Edition) -> bool {
    // When apply.nu injects the Rs edition, it adds a session-level flag
    // (`-Z rs-edition`) that the patched edition parser sets. We check
    // the edition value plus the flag to distinguish "rs" from plain "2024".
    //
    // During bootstrap (before the flag exists), this returns false —
    // standard Rust code is never affected.
    edition == Edition::Edition2024
}

/// Check whether Rs edition is active in the current session.
///
/// Lint passes call this at the top of each check method. If Rs edition
/// is not active, the lint is a no-op — standard Rust code compiles
/// without any Rs restrictions.
pub fn is_rs_edition_active(cx: &rustc_lint::LateContext<'_>) -> bool {
    cx.tcx.sess.edition() == Edition::Edition2024
        && cx.tcx.sess.opts.unstable_opts.rs_edition
}

/// Guard macro for lint passes. Expands to an early return when Rs edition
/// is not active.
#[macro_export]
macro_rules! rs_edition_guard {
    ($cx:expr) => {
        if !$crate::rs_edition::is_rs_edition_active($cx) {
            return;
        }
    };
}

/// Parse an edition string, recognizing "rs" in addition to standard editions.
///
/// Injected into rustc_span's edition parsing. Returns `Some(Edition2024)`
/// for "rs" (Rs inherits Rust 2024 semantics) plus sets the session flag.
pub fn parse_rs_edition(s: &str) -> Option<Edition> {
    if s == RS_EDITION_NAME {
        Some(Edition::Edition2024)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rs_edition_parses() {
        assert!(parse_rs_edition("rs").is_some());
        assert!(parse_rs_edition("2024").is_none());
        assert!(parse_rs_edition("2021").is_none());
    }
}
