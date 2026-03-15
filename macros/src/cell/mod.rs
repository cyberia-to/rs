//! `cell! {}` — the main cell declaration macro.
//!
//! Parses a cell definition DSL and generates:
//! 1. XxxState struct
//! 2. XxxStepState struct with StepReset
//! 3. Xxx wrapper struct
//! 4. Cell trait impl
//! 5. MigrateFrom impl (if migrate block present)
//! 6. Error enum (collected from Error::Variant usage) with From<Timeout>
//! 7. CellMetadata impl
//! 8. Public interface methods as impl block
//! 9. async(Duration) fn → with_deadline wrapping
//! 10. input/output channel declarations

mod codegen;
mod parse;

use proc_macro2::TokenStream;
use syn::Result;

pub fn expand(input: TokenStream) -> Result<TokenStream> {
    let cell_def = parse::parse_cell(input)?;
    codegen::generate(&cell_def)
}
