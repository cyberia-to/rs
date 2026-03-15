//! Register code generation — struct, Default, read/write/modify methods.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Result;

use super::{extract_type_name_str, Access, EnumInfo, RegisterDef, RegisterModuleAttr};

pub fn generate_register_code(
    reg: &RegisterDef,
    mod_attr: &RegisterModuleAttr,
    enums: &[EnumInfo],
) -> Result<TokenStream> {
    let name = &reg.name;
    let vis = &reg.vis;
    let doc_attrs = &reg.attrs;

    let addr = mod_attr.base + reg.offset;
    let raw_ty = width_to_type(reg.width);
    let raw_ty_ident = format_ident!("{}", raw_ty);

    let struct_fields = gen_struct_fields(reg);
    let default_fields = gen_default_fields(reg, enums);
    let unpack_fields = gen_unpack_fields(reg, enums, &raw_ty_ident);
    let pack_exprs = gen_pack_exprs(reg, &raw_ty_ident);

    let read_fn = gen_read_fn(reg, addr, &raw_ty_ident, &unpack_fields);
    let write_fn = gen_write_fn(reg, addr, &raw_ty_ident, &pack_exprs);
    let modify_fn = gen_modify_fn(reg, addr, &raw_ty_ident, &pack_exprs);

    Ok(quote! {
        #(#doc_attrs)*
        #[derive(Clone, Copy)]
        #vis struct #name {
            #(#struct_fields)*
        }

        impl Default for #name {
            fn default() -> Self {
                Self {
                    #(#default_fields)*
                }
            }
        }

        impl #name {
            #read_fn
            #write_fn
            #modify_fn
        }
    })
}

fn gen_struct_fields(reg: &RegisterDef) -> Vec<TokenStream> {
    reg.fields
        .iter()
        .map(|f| {
            let fname = &f.name;
            let fty = &f.ty;
            let fvis = &f.vis;
            quote! { #fvis #fname: #fty, }
        })
        .collect()
}

fn gen_default_fields(reg: &RegisterDef, _enums: &[EnumInfo]) -> Vec<TokenStream> {
    reg.fields
        .iter()
        .map(|f| {
            let fname = &f.name;
            let type_name = extract_type_name_str(&f.ty);
            let default_val = match type_name.as_deref() {
                Some("bool") => quote! { false },
                Some("u8") | Some("u16") | Some("u32") | Some("u64") => quote! { 0 },
                _ => quote! { Default::default() },
            };
            quote! { #fname: #default_val, }
        })
        .collect()
}

fn gen_unpack_fields(
    reg: &RegisterDef,
    enums: &[EnumInfo],
    _raw_ty_ident: &syn::Ident,
) -> Vec<TokenStream> {
    reg.fields
        .iter()
        .map(|f| {
            let fname = &f.name;
            let field_width = f.bit_end - f.bit_start;
            let mask = (1u64 << field_width) - 1;
            let shift = f.bit_start;
            let type_name = extract_type_name_str(&f.ty);

            let mask_lit = proc_macro2::Literal::u64_unsuffixed(mask);
            let shift_lit = proc_macro2::Literal::u32_unsuffixed(shift);

            match type_name.as_deref() {
                Some("bool") => {
                    quote! { #fname: (raw >> #shift_lit) & 1 != 0, }
                }
                Some("u8") => {
                    quote! { #fname: ((raw >> #shift_lit) & #mask_lit) as u8, }
                }
                Some("u16") => {
                    quote! { #fname: ((raw >> #shift_lit) & #mask_lit) as u16, }
                }
                Some("u32") => {
                    quote! { #fname: ((raw >> #shift_lit) & #mask_lit) as u32, }
                }
                Some("u64") => {
                    quote! { #fname: ((raw >> #shift_lit) & #mask_lit) as u64, }
                }
                Some(enum_name) if enums.iter().any(|e| e.name == enum_name) => {
                    let enum_info = enums.iter().find(|e| e.name == enum_name).unwrap();
                    let enum_ident = &enum_info.name;
                    let match_arms: Vec<TokenStream> = enum_info
                        .variants
                        .iter()
                        .map(|(vname, disc)| {
                            let d = disc.unwrap_or(0);
                            let d_lit = proc_macro2::Literal::u64_unsuffixed(d);
                            quote! { #d_lit => #enum_ident::#vname, }
                        })
                        .collect();
                    quote! {
                        #fname: match ((raw >> #shift_lit) & #mask_lit) as u64 {
                            #(#match_arms)*
                            _ => unsafe { ::core::hint::unreachable_unchecked() },
                        },
                    }
                }
                _ => {
                    quote! { #fname: ((raw >> #shift_lit) & #mask_lit) as _, }
                }
            }
        })
        .collect()
}

fn gen_pack_exprs(reg: &RegisterDef, raw_ty_ident: &syn::Ident) -> Vec<TokenStream> {
    reg.fields
        .iter()
        .map(|f| {
            let fname = &f.name;
            let field_width = f.bit_end - f.bit_start;
            let mask = (1u64 << field_width) - 1;
            let shift = f.bit_start;
            let type_name = extract_type_name_str(&f.ty);

            let mask_lit = proc_macro2::Literal::u64_unsuffixed(mask);
            let shift_lit = proc_macro2::Literal::u32_unsuffixed(shift);

            match type_name.as_deref() {
                Some("bool") => {
                    quote! { | ((val.#fname as #raw_ty_ident) << #shift_lit) }
                }
                _ => {
                    quote! { | (((val.#fname as #raw_ty_ident) & #mask_lit) << #shift_lit) }
                }
            }
        })
        .collect()
}

fn gen_read_fn(
    reg: &RegisterDef,
    addr: u64,
    raw_ty_ident: &syn::Ident,
    unpack_fields: &[TokenStream],
) -> TokenStream {
    if reg.access != Access::ReadOnly && reg.access != Access::ReadWrite {
        return TokenStream::new();
    }
    let addr_lit = proc_macro2::Literal::u64_unsuffixed(addr);
    quote! {
        #[inline(always)]
        pub fn read() -> Self {
            let raw: #raw_ty_ident = unsafe {
                ::core::ptr::read_volatile(#addr_lit as *const #raw_ty_ident)
            };
            Self {
                #(#unpack_fields)*
            }
        }
    }
}

fn gen_write_fn(
    reg: &RegisterDef,
    addr: u64,
    raw_ty_ident: &syn::Ident,
    pack_exprs: &[TokenStream],
) -> TokenStream {
    if reg.access != Access::WriteOnly && reg.access != Access::ReadWrite {
        return TokenStream::new();
    }
    let addr_lit = proc_macro2::Literal::u64_unsuffixed(addr);
    quote! {
        #[inline(always)]
        pub fn write<F: FnOnce(&mut Self)>(f: F) {
            let mut val = Self::default();
            f(&mut val);
            let raw: #raw_ty_ident = 0 #(#pack_exprs)*;
            unsafe {
                ::core::ptr::write_volatile(#addr_lit as *mut #raw_ty_ident, raw);
            }
        }
    }
}

fn gen_modify_fn(
    reg: &RegisterDef,
    addr: u64,
    raw_ty_ident: &syn::Ident,
    pack_exprs: &[TokenStream],
) -> TokenStream {
    if reg.access != Access::ReadWrite {
        return TokenStream::new();
    }
    let addr_lit = proc_macro2::Literal::u64_unsuffixed(addr);
    quote! {
        #[inline(always)]
        pub fn modify<F: FnOnce(&mut Self)>(f: F) {
            let mut val = Self::read();
            f(&mut val);
            let raw: #raw_ty_ident = 0 #(#pack_exprs)*;
            unsafe {
                ::core::ptr::write_volatile(#addr_lit as *mut #raw_ty_ident, raw);
            }
        }
    }
}

fn width_to_type(width: u32) -> &'static str {
    match width {
        8 => "u8",
        16 => "u16",
        32 => "u32",
        64 => "u64",
        _ => "u32",
    }
}
