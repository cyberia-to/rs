//! `#[deterministic]` — token-level checks for non-deterministic constructs.
//!
//! Walks the token stream of a function body and emits compile_error! for:
//! - RS201: f32 type paths
//! - RS202: f64 type paths
//! - RS204: std::time::Instant
//! - RS205: rand:: paths
//! - RS207: HashMap / HashSet paths
//! - RS208: asm! / global_asm! invocations
//!
//! RS206 (unchecked arithmetic) and RS209 (transitivity) require MIR analysis
//! and are enforced by rsc only.

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::quote;
use syn::{parse2, Error, ItemFn, Result};

/// Collected violation with its span and message.
struct Violation {
    span: Span,
    message: String,
}

pub fn expand(_attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
    let func: ItemFn = parse2(item.clone())?;
    let mut violations = Vec::new();

    // Scan the full token stream (including signature and body) for violations.
    scan_tokens(&item, &mut violations);

    if violations.is_empty() {
        // Emit original function with the attribute stripped (already consumed).
        Ok(quote! { #func })
    } else {
        // Emit compile errors followed by the original function so downstream
        // errors don't cascade from a missing definition.
        let errors: Vec<_> = violations
            .iter()
            .map(|v| {
                let msg = &v.message;
                Error::new(v.span, msg).into_compile_error()
            })
            .collect();
        Ok(quote! {
            #(#errors)*
            #func
        })
    }
}

/// Recursively scan a token stream for forbidden constructs.
fn scan_tokens(stream: &TokenStream, violations: &mut Vec<Violation>) {
    let tokens: Vec<TokenTree> = stream.clone().into_iter().collect();
    for (i, tt) in tokens.iter().enumerate() {
        match tt {
            TokenTree::Group(group) => {
                scan_tokens(&group.stream(), violations);
            }
            TokenTree::Ident(ident) => {
                let name = ident.to_string();

                // RS201/RS202: f32 / f64 type usage
                if name == "f32" {
                    violations.push(Violation {
                        span: ident.span(),
                        message: "RS201: f32 type used in #[deterministic] function; \
                                  use FixedPoint<u128, 18> for deterministic decimal arithmetic"
                            .into(),
                    });
                }
                if name == "f64" {
                    violations.push(Violation {
                        span: ident.span(),
                        message: "RS202: f64 type used in #[deterministic] function; \
                                  use FixedPoint<u128, 18> for deterministic decimal arithmetic"
                            .into(),
                    });
                }

                // RS207: HashMap / HashSet
                if name == "HashMap" {
                    violations.push(Violation {
                        span: ident.span(),
                        message: "RS207: HashMap used in #[deterministic] function; \
                                  HashMap iteration order is non-deterministic; use BTreeMap"
                            .into(),
                    });
                }
                if name == "HashSet" {
                    violations.push(Violation {
                        span: ident.span(),
                        message: "RS207: HashSet used in #[deterministic] function; \
                                  HashSet iteration order is non-deterministic; use BTreeSet"
                            .into(),
                    });
                }

                // RS205: rand:: path segment
                if name == "rand" && is_path_root(i, &tokens) {
                    violations.push(Violation {
                        span: ident.span(),
                        message: "RS205: rand used in #[deterministic] function; \
                                  randomness is non-deterministic by definition"
                            .into(),
                    });
                }

                // RS208: asm! / global_asm! macro invocations
                if name == "asm" && is_macro_call(i, &tokens) {
                    violations.push(Violation {
                        span: ident.span(),
                        message: "RS208: inline assembly in #[deterministic] function; \
                                  assembly is platform-specific by definition"
                            .into(),
                    });
                }
                if name == "global_asm" && is_macro_call(i, &tokens) {
                    violations.push(Violation {
                        span: ident.span(),
                        message: "RS208: global_asm in #[deterministic] function; \
                                  assembly is platform-specific by definition"
                            .into(),
                    });
                }

                // RS204: Instant (from std::time)
                if name == "Instant" {
                    // Check for std::time::Instant pattern or bare Instant usage
                    if is_std_time_instant(i, &tokens) || is_bare_instant(i, &tokens) {
                        violations.push(Violation {
                            span: ident.span(),
                            message: "RS204: std::time::Instant used in #[deterministic] \
                                      function; wall clock time is non-deterministic; \
                                      use step counters"
                                .into(),
                        });
                    }
                }
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

/// Check if ident at position `i` is followed by `::` (making it a path root).
fn is_path_root(i: usize, tokens: &[TokenTree]) -> bool {
    // Look for `ident ::` pattern
    if i + 2 < tokens.len() {
        if let (TokenTree::Punct(p1), TokenTree::Punct(p2)) = (&tokens[i + 1], &tokens[i + 2]) {
            return p1.as_char() == ':' && p2.as_char() == ':';
        }
    }
    false
}

/// Check if ident at position `i` is followed by `!` (making it a macro call).
fn is_macro_call(i: usize, tokens: &[TokenTree]) -> bool {
    if i + 1 < tokens.len() {
        if let TokenTree::Punct(p) = &tokens[i + 1] {
            return p.as_char() == '!';
        }
    }
    false
}

/// Check if `Instant` at position `i` is preceded by `std :: time ::`.
fn is_std_time_instant(i: usize, tokens: &[TokenTree]) -> bool {
    // Pattern: std :: time :: Instant
    // Positions:  i-6 i-5 i-4 i-3 i-2 i-1 i
    // Or:         ... :: time :: Instant
    if i >= 4 {
        let check_time = matches!(&tokens[i - 2], TokenTree::Ident(id) if id == "time");
        let check_colons_after_time =
            matches!(&tokens[i - 1], TokenTree::Punct(p) if p.as_char() == ':');
        if check_time && check_colons_after_time {
            return true;
        }
    }
    false
}

/// Bare `Instant` usage — if it appears as a type annotation or path segment
/// and there's no preceding `::` from a non-std path. We flag it conservatively.
fn is_bare_instant(i: usize, tokens: &[TokenTree]) -> bool {
    // If preceded by `:` (part of a type annotation `x: Instant`), flag it.
    if i >= 1 {
        if let TokenTree::Punct(p) = &tokens[i - 1] {
            if p.as_char() == ':' {
                // Check it's a single colon (type annotation), not ::
                if i >= 2 {
                    if let TokenTree::Punct(p2) = &tokens[i - 2] {
                        if p2.as_char() == ':' {
                            // This is `::Instant` — part of a path, check parent
                            return false;
                        }
                    }
                }
                return true;
            }
        }
    }

    // If followed by `::` it's being used as a path root (e.g. Instant::now())
    if is_path_root(i, tokens) {
        return true;
    }

    false
}

/// Walk a token group recursively, collecting all ident-level matches.
/// This is the entry point used by the cell macro to check deterministic
/// bodies without re-parsing as an ItemFn.
#[allow(dead_code)]
pub fn scan_token_stream(stream: &TokenStream) -> Vec<(Span, String)> {
    let mut violations = Vec::new();
    scan_tokens(stream, &mut violations);
    violations
        .into_iter()
        .map(|v| (v.span, v.message))
        .collect()
}
