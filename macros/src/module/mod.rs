//! `module! {}` — the main module declaration macro.
//!
//! Parses a module definition DSL and generates:
//! 1. XxxState struct
//! 2. XxxStepState struct with StepReset
//! 3. Xxx wrapper struct
//! 4. Module trait impl
//! 5. MigrateFrom impl (if migrate block present)
//! 6. Error enum (collected from Error::Variant usage) with From<Timeout>
//! 7. ModuleMetadata impl
//! 8. Public interface methods as impl block
//! 9. async(Duration) fn → with_deadline wrapping
//! 10. input/output channel declarations

mod codegen;
mod parse;

use proc_macro2::TokenStream;
use syn::Result;

pub fn expand(input: TokenStream) -> Result<TokenStream> {
    let module_def = parse::parse_module(input)?;
    codegen::generate(&module_def)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;
    #[test]
    fn unbounded_async_is_rejected_before_codegen() {
        let error = expand(quote! {
            name: Unbounded, version: 1, budget: Duration::from_secs(1),
            heartbeat: Duration::from_secs(1), state { count: u64 }
            pub async fn work(&self) -> Result<u64> { Ok(1) }
        }).unwrap_err();
        assert!(error.to_string().contains("RS101"));
    }
}
