//! Register validation — RS003, RS005, RS006, RS007, RS008.

use proc_macro2::Span;
use syn::{Error, Result};

use super::{extract_type_name_str, EnumInfo, RegisterDef, RegisterModuleAttr};

pub fn validate_register(
    reg: &RegisterDef,
    mod_attr: &RegisterModuleAttr,
    enums: &[EnumInfo],
) -> Result<()> {
    // RS007: offset within bank_size
    if reg.offset >= mod_attr.bank_size {
        return Err(Error::new(
            Span::call_site(),
            format!(
                "RS007: offset {:#x} exceeds bank_size {:#x}",
                reg.offset, mod_attr.bank_size
            ),
        ));
    }

    // RS003: field bit range within register width
    for field in &reg.fields {
        if field.bit_end > reg.width {
            return Err(Error::new_spanned(
                &field.name,
                format!(
                    "RS003: field {} (bits {}..{}) exceeds u{} width",
                    field.name, field.bit_start, field.bit_end, reg.width
                ),
            ));
        }
    }

    // RS005: overlapping field bits
    for (i, a) in reg.fields.iter().enumerate() {
        for b in reg.fields.iter().skip(i + 1) {
            if a.bit_start < b.bit_end && b.bit_start < a.bit_end {
                let overlap_bit = a.bit_start.max(b.bit_start);
                return Err(Error::new_spanned(
                    &b.name,
                    format!(
                        "RS005: fields {} and {} overlap at bit {}",
                        a.name, b.name, overlap_bit
                    ),
                ));
            }
        }
    }

    // RS006 / RS008: enum field checks
    for field in &reg.fields {
        let field_width = field.bit_end - field.bit_start;
        let type_name = extract_type_name_str(&field.ty);

        if let Some(name) = &type_name {
            if let Some(enum_info) = enums.iter().find(|e| e.name == *name) {
                let max_values = 1u64 << field_width;

                if enum_info.variant_count as u64 > max_values {
                    return Err(Error::new_spanned(
                        &field.name,
                        format!(
                            "RS006: {} has {} variants but field {} is {} bits (max {})",
                            enum_info.name,
                            enum_info.variant_count,
                            field.name,
                            field_width,
                            max_values
                        ),
                    ));
                }

                if (enum_info.variant_count as u64) < max_values {
                    let missing = max_values - enum_info.variant_count as u64;
                    return Err(Error::new_spanned(
                        &field.name,
                        format!(
                            "RS008: {} has {} variants but field {} is {} bits \
                             ({} patterns) — add a variant for {} missing pattern(s)",
                            enum_info.name,
                            enum_info.variant_count,
                            field.name,
                            field_width,
                            max_values,
                            missing
                        ),
                    ));
                }
            }
        }
    }

    Ok(())
}
