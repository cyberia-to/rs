//! Serialization code generation for Addressed derive.
//!
//! Generates the body of `serialize_canonical` and `serialized_size` methods
//! for each field type according to canonical serialization rules.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{GenericArgument, PathArguments, Type};

/// Generate serialization code for a field value.
pub fn serialize_field_expr(ty: &Type, accessor: &TokenStream) -> TokenStream {
    let type_name = extract_type_name(ty);

    match type_name.as_deref() {
        Some("u8") => quote! { buf.put_bytes(&[*&#accessor]); },
        Some("i8") => quote! { buf.put_bytes(&[*&#accessor as u8]); },
        Some("u16") => quote! { buf.put_bytes(&(#accessor).to_le_bytes()); },
        Some("i16") => quote! { buf.put_bytes(&(#accessor).to_le_bytes()); },
        Some("u32") => quote! { buf.put_bytes(&(#accessor).to_le_bytes()); },
        Some("i32") => quote! { buf.put_bytes(&(#accessor).to_le_bytes()); },
        Some("u64") => quote! { buf.put_bytes(&(#accessor).to_le_bytes()); },
        Some("i64") => quote! { buf.put_bytes(&(#accessor).to_le_bytes()); },
        Some("u128") => quote! { buf.put_bytes(&(#accessor).to_le_bytes()); },
        Some("i128") => quote! { buf.put_bytes(&(#accessor).to_le_bytes()); },

        Some("bool") => quote! {
            buf.put_bytes(&[if #accessor { 1u8 } else { 0u8 }]);
        },

        Some("Option") => {
            if let Some(inner_ty) = extract_first_generic_arg(ty) {
                let inner_serialize = serialize_field_expr(inner_ty, &quote! { val });
                quote! {
                    match &#accessor {
                        Some(val) => {
                            buf.put_bytes(&[1u8]);
                            #inner_serialize
                        }
                        None => {
                            buf.put_bytes(&[0u8]);
                        }
                    }
                }
            } else {
                quote! {
                    rs_lang::CanonicalSerialize::serialize_canonical(&#accessor, buf);
                }
            }
        }

        Some("BoundedVec") => {
            if let Some(inner_ty) = extract_first_generic_arg(ty) {
                let elem_serialize = serialize_field_expr(inner_ty, &quote! { elem });
                quote! {
                    buf.put_bytes(&((#accessor).len() as u32).to_le_bytes());
                    for elem in (#accessor).iter() {
                        #elem_serialize
                    }
                }
            } else {
                quote! {
                    rs_lang::CanonicalSerialize::serialize_canonical(&#accessor, buf);
                }
            }
        }

        Some("BoundedMap") => {
            quote! {
                rs_lang::CanonicalSerialize::serialize_canonical(&#accessor, buf);
            }
        }

        _ => {
            if let Type::Array(arr) = ty {
                let elem_ty = &*arr.elem;
                let elem_serialize = serialize_field_expr(elem_ty, &quote! { (*__elem) });
                return quote! {
                    for __elem in (#accessor).iter() {
                        #elem_serialize
                    }
                };
            }

            quote! {
                rs_lang::CanonicalSerialize::serialize_canonical(&#accessor, buf);
            }
        }
    }
}

/// Generate serialized_size expression for a field.
pub fn serialized_size_expr(ty: &Type, accessor: &TokenStream) -> TokenStream {
    let type_name = extract_type_name(ty);

    match type_name.as_deref() {
        Some("u8") | Some("i8") => quote! { 1usize },
        Some("bool") => quote! { 1usize },
        Some("u16") | Some("i16") => quote! { 2usize },
        Some("u32") | Some("i32") => quote! { 4usize },
        Some("u64") | Some("i64") => quote! { 8usize },
        Some("u128") | Some("i128") => quote! { 16usize },

        Some("Option") => {
            if let Some(inner_ty) = extract_first_generic_arg(ty) {
                let inner_size = serialized_size_expr(inner_ty, &quote! { val });
                quote! {
                    match &#accessor {
                        Some(val) => 1usize + #inner_size,
                        None => 1usize,
                    }
                }
            } else {
                quote! { rs_lang::CanonicalSerialize::serialized_size(&#accessor) }
            }
        }

        Some("BoundedVec") => {
            if let Some(inner_ty) = extract_first_generic_arg(ty) {
                let elem_size = serialized_size_expr(inner_ty, &quote! { elem });
                quote! {
                    {
                        let mut __sz = 4usize;
                        for elem in (#accessor).iter() {
                            __sz += #elem_size;
                        }
                        __sz
                    }
                }
            } else {
                quote! { rs_lang::CanonicalSerialize::serialized_size(&#accessor) }
            }
        }

        Some("BoundedMap") => {
            quote! { rs_lang::CanonicalSerialize::serialized_size(&#accessor) }
        }

        _ => {
            if let Type::Array(arr) = ty {
                let elem_ty = &*arr.elem;
                let elem_size = serialized_size_expr(elem_ty, &quote! { (*__elem) });
                return quote! {
                    {
                        let mut __sz = 0usize;
                        for __elem in (#accessor).iter() {
                            __sz += #elem_size;
                        }
                        __sz
                    }
                };
            }

            quote! { rs_lang::CanonicalSerialize::serialized_size(&#accessor) }
        }
    }
}

pub fn extract_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(type_path) => {
            let last_seg = type_path.path.segments.last()?;
            Some(last_seg.ident.to_string())
        }
        _ => None,
    }
}

pub fn extract_first_generic_arg(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Path(type_path) => {
            let last_seg = type_path.path.segments.last()?;
            if let PathArguments::AngleBracketed(args) = &last_seg.arguments {
                for arg in &args.args {
                    if let GenericArgument::Type(inner) = arg {
                        return Some(inner);
                    }
                }
            }
            None
        }
        _ => None,
    }
}
