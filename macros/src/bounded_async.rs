//! `#[bounded_async(Duration)]` — wraps an async fn body with a deadline.
//!
//! Desugars:
//! ```ignore
//! #[bounded_async(Duration::from_millis(100))]
//! async fn fetch(id: u64) -> Result<Item, AppError> { body }
//! ```
//! into:
//! ```ignore
//! fn fetch(id: u64) -> impl ::core::future::Future<Output = Result<Item, AppError>> {
//!     rs_lang::runtime::with_deadline(Duration::from_millis(100), async move { body })
//! }
//! ```

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse2, Error, ItemFn, Result, ReturnType};

pub fn expand(attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
    let deadline_expr: syn::Expr = parse2(attr)?;
    let func: ItemFn = parse2(item)?;

    if func.sig.asyncness.is_none() {
        return Err(Error::new_spanned(
            &func.sig.fn_token,
            "#[bounded_async] can only be applied to async functions",
        ));
    }

    let vis = &func.vis;
    let name = &func.sig.ident;
    let generics = &func.sig.generics;
    let where_clause = &func.sig.generics.where_clause;
    let inputs = &func.sig.inputs;
    let attrs = &func.attrs;
    let body = &func.block;

    let output_ty = match &func.sig.output {
        ReturnType::Default => {
            return Err(Error::new_spanned(
                &func.sig,
                "#[bounded_async] functions must return Result<T, E> \
                 where E: From<rs_lang::Timeout>",
            ));
        }
        ReturnType::Type(_, ty) => ty,
    };

    Ok(quote! {
        #(#attrs)*
        #vis fn #name #generics (#inputs) -> impl ::core::future::Future<Output = #output_ty> #where_clause {
            rs_lang::runtime::with_deadline(#deadline_expr, async move #body)
        }
    })
}
