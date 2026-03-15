//! Type validation for Addressed derive.
//!
//! Rejects types that cannot be canonically serialized:
//! - RS302: f32/f64 fields
//! - RS303: *const T / *mut T
//! - RS304: HashMap
//! - RS305: usize/isize
//! - RS306: #[repr(u64/i64/u128)] on enums

use syn::{DeriveInput, Error, GenericArgument, PathArguments, Result, Type};

pub fn validate_type(ty: &Type) -> Result<()> {
    match ty {
        Type::Path(type_path) => {
            let last_seg = type_path.path.segments.last();
            if let Some(seg) = last_seg {
                let name = seg.ident.to_string();

                if name == "f32" || name == "f64" {
                    return Err(Error::new_spanned(
                        ty,
                        "RS302: floating point types are not canonically serializable; \
                         use FixedPoint<u128, 18> for deterministic decimal values",
                    ));
                }

                if name == "HashMap" {
                    return Err(Error::new_spanned(
                        ty,
                        "RS304: HashMap has non-deterministic serialization; \
                         use BTreeMap or BoundedMap",
                    ));
                }

                if name == "usize" || name == "isize" {
                    return Err(Error::new_spanned(
                        ty,
                        "RS305: usize/isize width is platform-dependent; \
                         use u32 or u64",
                    ));
                }

                if let PathArguments::AngleBracketed(args) = &seg.arguments {
                    for arg in &args.args {
                        if let GenericArgument::Type(inner_ty) = arg {
                            validate_type(inner_ty)?;
                        }
                    }
                }
            }

            // Recurse into all path segments (catches std::collections::HashMap).
            for seg in &type_path.path.segments {
                if let PathArguments::AngleBracketed(args) = &seg.arguments {
                    for arg in &args.args {
                        if let GenericArgument::Type(inner_ty) = arg {
                            validate_type(inner_ty)?;
                        }
                    }
                }
                let name = seg.ident.to_string();
                if name == "HashMap" {
                    return Err(Error::new_spanned(
                        ty,
                        "RS304: HashMap has non-deterministic serialization; \
                         use BTreeMap or BoundedMap",
                    ));
                }
            }
            Ok(())
        }

        Type::Ptr(_) => Err(Error::new_spanned(
            ty,
            "RS303: pointers cannot be addressed; \
             pointers are memory addresses, not content",
        )),

        Type::Array(arr) => validate_type(&arr.elem),
        Type::Slice(sl) => validate_type(&sl.elem),

        Type::Tuple(tup) => {
            for elem in &tup.elems {
                validate_type(elem)?;
            }
            Ok(())
        }

        Type::Reference(reference) => validate_type(&reference.elem),

        _ => Ok(()),
    }
}

pub fn validate_enum_repr(input: &DeriveInput) -> Result<()> {
    for attr in &input.attrs {
        if attr.path().is_ident("repr") {
            let repr_str = attr
                .parse_args::<syn::Ident>()
                .map(|id| id.to_string())
                .unwrap_or_default();
            match repr_str.as_str() {
                "u64" | "i64" | "u128" | "i128" => {
                    return Err(Error::new_spanned(
                        attr,
                        "RS306: Addressed enum discriminant must fit in u32; \
                         #[repr(u64)] / #[repr(i64)] / #[repr(u128)] is not supported",
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}
