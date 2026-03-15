//! `#[register(base, bank_size, width)]` — typed MMIO register code generation.
//!
//! Parses a module annotated with `#[register(...)]` containing `#[reg(...)]`
//! structs and enums. Validates bit layouts and generates safe read/write/modify
//! methods backed by read_volatile/write_volatile.
//!
//! Errors: RS001-RS008 (see reference/errors/registers.md).

mod codegen;
mod validate;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    parse::Parser, parse2, punctuated::Punctuated, Attribute, Error, Expr, ExprLit, Field, Fields,
    Ident, Item, ItemEnum, ItemMod, ItemStruct, Lit, LitInt, Result, Token, Type, Visibility,
};

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

pub(crate) struct RegisterModuleAttr {
    pub base: u64,
    pub bank_size: u64,
    pub width: u32,
}

pub(crate) struct RegAttr {
    pub offset: u64,
    pub access: Access,
    pub width_override: Option<u32>,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Access {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

pub(crate) struct FieldInfo {
    pub name: Ident,
    pub ty: Type,
    pub bit_start: u32,
    pub bit_end: u32,
    pub vis: Visibility,
}

pub(crate) struct EnumInfo {
    pub name: Ident,
    pub variant_count: usize,
    pub variants: Vec<(Ident, Option<u64>)>,
}

pub(crate) struct RegisterDef {
    pub name: Ident,
    pub vis: Visibility,
    pub attrs: Vec<Attribute>,
    pub offset: u64,
    pub access: Access,
    pub width: u32,
    pub fields: Vec<FieldInfo>,
}

// ---------------------------------------------------------------------------
// Main expansion
// ---------------------------------------------------------------------------

pub fn expand(attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
    let mod_attr = parse_register_module_attr(attr)?;
    let module: ItemMod = parse2(item)?;

    let mod_name = &module.ident;
    let mod_vis = &module.vis;

    let content = match &module.content {
        Some((_, items)) => items,
        None => {
            return Err(Error::new_spanned(
                &module,
                "#[register] must be applied to a module with a body",
            ));
        }
    };

    let enums = collect_enums(content)?;

    let mut generated_items = Vec::new();
    let mut passthrough_items = Vec::new();

    for item in content {
        match item {
            Item::Struct(s) => {
                if let Some(reg_attr) = extract_reg_attr(&s.attrs)? {
                    let reg = parse_register_struct(s, &reg_attr, &mod_attr)?;
                    validate::validate_register(&reg, &mod_attr, &enums)?;
                    let gen = codegen::generate_register_code(&reg, &mod_attr, &enums)?;
                    generated_items.push(gen);
                } else {
                    passthrough_items.push(quote! { #s });
                }
            }
            Item::Enum(e) => {
                passthrough_items.push(generate_enum_passthrough(e)?);
            }
            other => {
                passthrough_items.push(quote! { #other });
            }
        }
    }

    let mod_attrs: Vec<_> = module
        .attrs
        .iter()
        .filter(|a| !a.path().is_ident("register"))
        .collect();

    Ok(quote! {
        #(#mod_attrs)*
        #mod_vis mod #mod_name {
            #(#passthrough_items)*
            #(#generated_items)*
        }
    })
}

// ---------------------------------------------------------------------------
// Attribute parsing
// ---------------------------------------------------------------------------

fn parse_register_module_attr(attr: TokenStream) -> Result<RegisterModuleAttr> {
    let mut base: Option<u64> = None;
    let mut bank_size: Option<u64> = None;
    let mut width: Option<u32> = None;

    let parser = Punctuated::<syn::MetaNameValue, Token![,]>::parse_terminated;
    let parsed = parser.parse2(attr)?;

    for nv in &parsed {
        let key = nv
            .path
            .get_ident()
            .ok_or_else(|| Error::new_spanned(&nv.path, "expected identifier"))?
            .to_string();
        match key.as_str() {
            "base" => base = Some(expr_to_u64(&nv.value)?),
            "bank_size" => bank_size = Some(expr_to_u64(&nv.value)?),
            "width" => width = Some(expr_to_u32(&nv.value)?),
            _ => {
                return Err(Error::new_spanned(
                    &nv.path,
                    "expected `base`, `bank_size`, or `width`",
                ));
            }
        }
    }

    Ok(RegisterModuleAttr {
        base: base.ok_or_else(|| Error::new(Span::call_site(), "missing `base` in #[register]"))?,
        bank_size: bank_size
            .ok_or_else(|| Error::new(Span::call_site(), "missing `bank_size` in #[register]"))?,
        width: width.unwrap_or(32),
    })
}

pub(crate) fn parse_u64_lit(lit: &LitInt) -> Result<u64> {
    let s = lit.to_string().replace('_', "");
    if s.starts_with("0x") || s.starts_with("0X") {
        u64::from_str_radix(&s[2..], 16).map_err(|e| Error::new(lit.span(), e))
    } else if s.starts_with("0b") || s.starts_with("0B") {
        u64::from_str_radix(&s[2..], 2).map_err(|e| Error::new(lit.span(), e))
    } else if s.starts_with("0o") || s.starts_with("0O") {
        u64::from_str_radix(&s[2..], 8).map_err(|e| Error::new(lit.span(), e))
    } else {
        s.parse::<u64>().map_err(|e| Error::new(lit.span(), e))
    }
}

fn expr_to_u64(expr: &Expr) -> Result<u64> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Int(lit), .. }) => parse_u64_lit(lit),
        _ => Err(Error::new_spanned(expr, "expected integer literal")),
    }
}

fn expr_to_u32(expr: &Expr) -> Result<u32> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Int(lit), .. }) => lit.base10_parse(),
        _ => Err(Error::new_spanned(expr, "expected integer literal")),
    }
}

// ---------------------------------------------------------------------------
// Register struct parsing
// ---------------------------------------------------------------------------

fn extract_reg_attr(attrs: &[Attribute]) -> Result<Option<RegAttr>> {
    for attr in attrs {
        if attr.path().is_ident("reg") {
            let mut offset: Option<u64> = None;
            let mut access: Option<Access> = None;
            let mut width_override: Option<u32> = None;

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("offset") {
                    let value: LitInt = meta.value()?.parse()?;
                    offset = Some(parse_u64_lit(&value)?);
                    Ok(())
                } else if meta.path.is_ident("access") {
                    let value: syn::LitStr = meta.value()?.parse()?;
                    access = Some(match value.value().as_str() {
                        "ro" => Access::ReadOnly,
                        "wo" => Access::WriteOnly,
                        "rw" => Access::ReadWrite,
                        other => {
                            return Err(Error::new(
                                value.span(),
                                format!(
                                    "unknown access mode `{}`; expected ro, wo, or rw",
                                    other
                                ),
                            ))
                        }
                    });
                    Ok(())
                } else if meta.path.is_ident("width") {
                    let value: LitInt = meta.value()?.parse()?;
                    width_override = Some(value.base10_parse()?);
                    Ok(())
                } else {
                    Err(meta.error("expected `offset`, `access`, or `width`"))
                }
            })?;

            return Ok(Some(RegAttr {
                offset: offset
                    .ok_or_else(|| Error::new_spanned(attr, "missing `offset` in #[reg]"))?,
                access: access
                    .ok_or_else(|| Error::new_spanned(attr, "missing `access` in #[reg]"))?,
                width_override,
            }));
        }
    }
    Ok(None)
}

fn parse_register_struct(
    s: &ItemStruct,
    reg_attr: &RegAttr,
    mod_attr: &RegisterModuleAttr,
) -> Result<RegisterDef> {
    let reg_width = reg_attr.width_override.unwrap_or(mod_attr.width);

    let fields = match &s.fields {
        Fields::Named(named) => {
            let mut result = Vec::new();
            for f in &named.named {
                if let Some(fi) = parse_field_attr(f)? {
                    result.push(fi);
                }
            }
            result
        }
        _ => {
            return Err(Error::new_spanned(s, "#[reg] structs must have named fields"));
        }
    };

    Ok(RegisterDef {
        name: s.ident.clone(),
        vis: s.vis.clone(),
        attrs: s.attrs.iter().filter(|a| !a.path().is_ident("reg")).cloned().collect(),
        offset: reg_attr.offset,
        access: reg_attr.access,
        width: reg_width,
        fields,
    })
}

fn parse_field_attr(f: &Field) -> Result<Option<FieldInfo>> {
    for attr in &f.attrs {
        if attr.path().is_ident("field") {
            let mut bit_start: Option<u32> = None;
            let mut bit_end: Option<u32> = None;

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("bits") {
                    let value = meta.value()?;
                    let start_lit: LitInt = value.parse()?;
                    let start: u32 = start_lit.base10_parse()?;
                    let _dot_dot: Token![..] = value.parse()?;
                    let end_lit: LitInt = value.parse()?;
                    let end: u32 = end_lit.base10_parse()?;
                    bit_start = Some(start);
                    bit_end = Some(end);
                    Ok(())
                } else {
                    Err(meta.error("expected `bits`"))
                }
            })?;

            let start =
                bit_start.ok_or_else(|| Error::new_spanned(attr, "missing `bits` in #[field]"))?;
            let end =
                bit_end.ok_or_else(|| Error::new_spanned(attr, "missing `bits` in #[field]"))?;

            return Ok(Some(FieldInfo {
                name: f.ident.clone().unwrap(),
                ty: f.ty.clone(),
                bit_start: start,
                bit_end: end,
                vis: f.vis.clone(),
            }));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Enum helpers
// ---------------------------------------------------------------------------

fn collect_enums(items: &[Item]) -> Result<Vec<EnumInfo>> {
    let mut enums = Vec::new();
    for item in items {
        if let Item::Enum(e) = item {
            let mut variants = Vec::new();
            for v in &e.variants {
                let disc = match &v.discriminant {
                    Some((_, Expr::Lit(ExprLit { lit: Lit::Int(lit), .. }))) => {
                        Some(parse_u64_lit(lit)?)
                    }
                    _ => None,
                };
                variants.push((v.ident.clone(), disc));
            }
            enums.push(EnumInfo {
                name: e.ident.clone(),
                variant_count: e.variants.len(),
                variants,
            });
        }
    }
    Ok(enums)
}

fn generate_enum_passthrough(e: &ItemEnum) -> Result<TokenStream> {
    let attrs = &e.attrs;
    let vis = &e.vis;
    let name = &e.ident;
    let variants = &e.variants;
    let first_variant = variants.first().map(|v| &v.ident);

    Ok(quote! {
        #(#attrs)*
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #vis enum #name { #variants }

        impl Default for #name {
            fn default() -> Self {
                #name::#first_variant
            }
        }
    })
}

pub(crate) fn extract_type_name_str(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(tp) => Some(tp.path.segments.last()?.ident.to_string()),
        _ => None,
    }
}
