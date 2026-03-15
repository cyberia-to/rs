//! `#[derive(Addressed)]` — canonical serialization and particle identity.
//!
//! Generates:
//! - `impl CanonicalSerialize for X` with fields serialized in declaration order
//! - `fn particle(&self) -> rs_lang::Particle`
//!
//! Compile-time rejection:
//! - RS302: f32/f64 fields
//! - RS303: *const T / *mut T
//! - RS304: HashMap
//! - RS305: usize/isize
//! - RS306: #[repr(u64/i64/u128)] on enums

mod serialize;
mod validate;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse2, Data, DeriveInput, Error, Fields, Result};

use serialize::{serialize_field_expr, serialized_size_expr};
use validate::{validate_enum_repr, validate_type};

pub fn derive(input: TokenStream) -> Result<TokenStream> {
    let input: DeriveInput = parse2(input)?;

    match &input.data {
        Data::Struct(_) => derive_struct(&input),
        Data::Enum(_) => derive_enum(&input),
        Data::Union(_) => Err(Error::new_spanned(
            &input.ident,
            "Addressed cannot be derived for unions",
        )),
    }
}

// ---------------------------------------------------------------------------
// Struct derivation
// ---------------------------------------------------------------------------

fn derive_struct(input: &DeriveInput) -> Result<TokenStream> {
    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        _ => unreachable!(),
    };

    let named_fields = match fields {
        Fields::Named(named) => &named.named,
        Fields::Unnamed(unnamed) => &unnamed.unnamed,
        Fields::Unit => {
            return Ok(generate_unit_struct_impl(input));
        }
    };

    for field in named_fields.iter() {
        validate_type(&field.ty)?;
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let serialize_stmts: Vec<TokenStream> = named_fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let accessor = match &field.ident {
                Some(ident) => quote! { self.#ident },
                None => {
                    let idx = syn::Index::from(i);
                    quote! { self.#idx }
                }
            };
            serialize_field_expr(&field.ty, &accessor)
        })
        .collect();

    let size_stmts: Vec<TokenStream> = named_fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let accessor = match &field.ident {
                Some(ident) => quote! { self.#ident },
                None => {
                    let idx = syn::Index::from(i);
                    quote! { self.#idx }
                }
            };
            serialized_size_expr(&field.ty, &accessor)
        })
        .collect();

    Ok(quote! {
        impl #impl_generics rs_lang::CanonicalSerialize for #name #ty_generics #where_clause {
            fn serialize_canonical(&self, buf: &mut impl rs_lang::BufMut) {
                #(#serialize_stmts)*
            }

            fn serialized_size(&self) -> usize {
                let mut size = 0usize;
                #(size += #size_stmts;)*
                size
            }
        }

        impl #impl_generics #name #ty_generics #where_clause {
            /// Compute the content-address (Particle) of this value.
            pub fn particle(&self) -> rs_lang::Particle {
                rs_lang::Particle::from_canonical(self)
            }
        }
    })
}

fn generate_unit_struct_impl(input: &DeriveInput) -> TokenStream {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics rs_lang::CanonicalSerialize for #name #ty_generics #where_clause {
            fn serialize_canonical(&self, _buf: &mut impl rs_lang::BufMut) {}
            fn serialized_size(&self) -> usize { 0 }
        }

        impl #impl_generics #name #ty_generics #where_clause {
            pub fn particle(&self) -> rs_lang::Particle {
                rs_lang::Particle::from_bytes(&[])
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Enum derivation
// ---------------------------------------------------------------------------

fn derive_enum(input: &DeriveInput) -> Result<TokenStream> {
    let data = match &input.data {
        Data::Enum(data) => data,
        _ => unreachable!(),
    };

    validate_enum_repr(input)?;

    for variant in &data.variants {
        match &variant.fields {
            Fields::Named(fields) => {
                for f in &fields.named {
                    validate_type(&f.ty)?;
                }
            }
            Fields::Unnamed(fields) => {
                for f in &fields.unnamed {
                    validate_type(&f.ty)?;
                }
            }
            Fields::Unit => {}
        }
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let serialize_arms = gen_serialize_arms(name, data);
    let size_arms = gen_size_arms(name, data);

    Ok(quote! {
        impl #impl_generics rs_lang::CanonicalSerialize for #name #ty_generics #where_clause {
            fn serialize_canonical(&self, buf: &mut impl rs_lang::BufMut) {
                match self {
                    #(#serialize_arms)*
                }
            }

            fn serialized_size(&self) -> usize {
                match self {
                    #(#size_arms)*
                }
            }
        }

        impl #impl_generics #name #ty_generics #where_clause {
            pub fn particle(&self) -> rs_lang::Particle {
                rs_lang::Particle::from_canonical(self)
            }
        }
    })
}

fn gen_serialize_arms(
    name: &syn::Ident,
    data: &syn::DataEnum,
) -> Vec<TokenStream> {
    data.variants
        .iter()
        .enumerate()
        .map(|(disc_idx, variant)| {
            let vname = &variant.ident;
            let disc = disc_idx as u32;

            match &variant.fields {
                Fields::Unit => {
                    quote! {
                        #name::#vname => {
                            buf.put_bytes(&(#disc as u32).to_le_bytes());
                        }
                    }
                }
                Fields::Unnamed(fields) => {
                    let bindings: Vec<_> = (0..fields.unnamed.len())
                        .map(|i| format_ident!("__f{}", i))
                        .collect();
                    let stmts: Vec<_> = fields
                        .unnamed
                        .iter()
                        .zip(bindings.iter())
                        .map(|(f, b)| serialize_field_expr(&f.ty, &quote! { #b }))
                        .collect();
                    quote! {
                        #name::#vname(#(ref #bindings),*) => {
                            buf.put_bytes(&(#disc as u32).to_le_bytes());
                            #(#stmts)*
                        }
                    }
                }
                Fields::Named(fields) => {
                    let field_names: Vec<_> =
                        fields.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
                    let stmts: Vec<_> = fields
                        .named
                        .iter()
                        .zip(field_names.iter())
                        .map(|(f, fname)| serialize_field_expr(&f.ty, &quote! { #fname }))
                        .collect();
                    quote! {
                        #name::#vname { #(ref #field_names),* } => {
                            buf.put_bytes(&(#disc as u32).to_le_bytes());
                            #(#stmts)*
                        }
                    }
                }
            }
        })
        .collect()
}

fn gen_size_arms(
    name: &syn::Ident,
    data: &syn::DataEnum,
) -> Vec<TokenStream> {
    data.variants
        .iter()
        .map(|variant| {
            let vname = &variant.ident;
            match &variant.fields {
                Fields::Unit => {
                    quote! { #name::#vname => { 4 } }
                }
                Fields::Unnamed(fields) => {
                    let bindings: Vec<_> = (0..fields.unnamed.len())
                        .map(|i| format_ident!("__f{}", i))
                        .collect();
                    let size_exprs: Vec<_> = fields
                        .unnamed
                        .iter()
                        .zip(bindings.iter())
                        .map(|(f, b)| serialized_size_expr(&f.ty, &quote! { #b }))
                        .collect();
                    quote! {
                        #name::#vname(#(ref #bindings),*) => {
                            let mut s = 4usize;
                            #(s += #size_exprs;)*
                            s
                        }
                    }
                }
                Fields::Named(fields) => {
                    let field_names: Vec<_> =
                        fields.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
                    let size_exprs: Vec<_> = fields
                        .named
                        .iter()
                        .zip(field_names.iter())
                        .map(|(f, fname)| serialized_size_expr(&f.ty, &quote! { #fname }))
                        .collect();
                    quote! {
                        #name::#vname { #(ref #field_names),* } => {
                            let mut s = 4usize;
                            #(s += #size_exprs;)*
                            s
                        }
                    }
                }
            }
        })
        .collect()
}
