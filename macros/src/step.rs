//! `#[step]` — generates `StepReset` impl for structs.
//!
//! Reset rules by field type:
//! - Integer types (u8..u128, i8..i128) → 0
//! - bool → false
//! - Option<T> → None
//! - BoundedVec<T, N> / BoundedMap<K, V, N> → clear()
//! - AtomicU32 / AtomicU64 → store(0, Ordering::SeqCst)
//! - Custom types → StepReset::reset(&mut self)

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    parse2, Data, DeriveInput, Error, Fields, Result, Type,
};

pub fn expand(_attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
    let input: DeriveInput = parse2(item)?;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            Fields::Unnamed(_) => {
                return Err(Error::new_spanned(
                    &input.ident,
                    "#[step] does not support tuple structs",
                ));
            }
            Fields::Unit => {
                return Err(Error::new_spanned(
                    &input.ident,
                    "#[step] does not support unit structs",
                ));
            }
        },
        _ => {
            return Err(Error::new_spanned(
                &input.ident,
                "#[step] can only be applied to structs with named fields",
            ));
        }
    };

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let reset_stmts: Vec<TokenStream> = fields
        .iter()
        .map(|f| {
            let field_name = f.ident.as_ref().unwrap();
            let reset = reset_expr_for_type(&f.ty, field_name);
            reset
        })
        .collect();

    let attrs = &input.attrs;
    let vis = &input.vis;
    let generics = &input.generics;
    let fields_def = match &input.data {
        Data::Struct(data) => &data.fields,
        _ => unreachable!(),
    };

    Ok(quote! {
        #(#attrs)*
        #vis struct #name #generics #where_clause #fields_def

        impl #impl_generics rs_lang::StepReset for #name #ty_generics #where_clause {
            fn reset(&mut self) {
                #(#reset_stmts)*
            }
        }
    })
}

/// Generate a reset statement for a single field based on its type.
fn reset_expr_for_type(ty: &Type, field_name: &syn::Ident) -> TokenStream {
    let type_name = extract_type_name(ty);

    match type_name.as_deref() {
        // Integer types → 0
        Some("u8") | Some("u16") | Some("u32") | Some("u64") | Some("u128")
        | Some("i8") | Some("i16") | Some("i32") | Some("i64") | Some("i128") => {
            quote! { self.#field_name = 0; }
        }

        // bool → false
        Some("bool") => {
            quote! { self.#field_name = false; }
        }

        // Option<T> → None
        Some("Option") => {
            quote! { self.#field_name = None; }
        }

        // BoundedVec / BoundedMap → clear()
        Some("BoundedVec") | Some("BoundedMap") => {
            quote! { self.#field_name.clear(); }
        }

        // AtomicU32 / AtomicU64 → store(0)
        Some("AtomicU32") | Some("AtomicU64") => {
            quote! {
                self.#field_name.store(0, ::core::sync::atomic::Ordering::SeqCst);
            }
        }

        // Custom types → delegate to StepReset::reset
        _ => {
            quote! {
                rs_lang::StepReset::reset(&mut self.#field_name);
            }
        }
    }
}

/// Extract the outermost type name from a Type node.
fn extract_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(type_path) => {
            let last_seg = type_path.path.segments.last()?;
            Some(last_seg.ident.to_string())
        }
        _ => None,
    }
}
