//! Cell code generation.
//!
//! Takes a parsed `CellDef` and generates all output code:
//! state structs, wrapper, Cell trait, migration, error enum,
//! metadata, methods, and channel type aliases.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Ident, Result, ReturnType, Type};

use super::parse::{CellDef, CellField, CellMethod, MethodVis, MigrateSource, SelfArg};

pub fn generate(cell: &CellDef) -> Result<TokenStream> {
    let state_struct = gen_state_struct(cell);
    let step_state_struct = gen_step_state_struct(cell);
    let wrapper_struct = gen_wrapper_struct(cell);
    let cell_trait_impl = gen_cell_trait_impl(cell);
    let migrate_impl = gen_migrate_impl(cell);
    let error_enum = gen_error_enum(cell);
    let error_aliases = gen_error_aliases(cell);
    let metadata_impl = gen_metadata_impl(cell);
    let methods_impl = gen_methods_impl(cell);
    let channel_types = gen_channel_types(cell);

    Ok(quote! {
        #state_struct
        #step_state_struct
        #wrapper_struct
        #cell_trait_impl
        #migrate_impl
        #error_enum
        #error_aliases
        #metadata_impl
        #methods_impl
        #channel_types
    })
}

fn gen_state_struct(cell: &CellDef) -> TokenStream {
    let state_name = format_ident!("{}State", cell.name);
    let fields: Vec<TokenStream> = cell
        .state_fields
        .iter()
        .map(|f| {
            let name = &f.name;
            let ty = &f.ty;
            quote! { pub #name: #ty, }
        })
        .collect();
    quote! {
        pub struct #state_name {
            #(#fields)*
        }
    }
}

fn gen_step_state_struct(cell: &CellDef) -> TokenStream {
    let step_name = format_ident!("{}StepState", cell.name);
    if cell.step_state_fields.is_empty() {
        return quote! {
            #[step]
            pub struct #step_name;
            impl rs_lang::StepReset for #step_name {
                fn reset(&mut self) {}
            }
        };
    }
    let fields: Vec<TokenStream> = cell
        .step_state_fields
        .iter()
        .map(|f| {
            let name = &f.name;
            let ty = &f.ty;
            quote! { pub #name: #ty, }
        })
        .collect();
    let reset_stmts: Vec<TokenStream> = cell
        .step_state_fields
        .iter()
        .map(|f| step_reset_stmt(f))
        .collect();
    quote! {
        pub struct #step_name {
            #(#fields)*
        }
        impl rs_lang::StepReset for #step_name {
            fn reset(&mut self) {
                #(#reset_stmts)*
            }
        }
    }
}

fn step_reset_stmt(field: &CellField) -> TokenStream {
    let name = &field.name;
    let type_name = extract_type_name(&field.ty);
    match type_name.as_deref() {
        Some("u8") | Some("u16") | Some("u32") | Some("u64") | Some("u128")
        | Some("i8") | Some("i16") | Some("i32") | Some("i64") | Some("i128") => {
            quote! { self.#name = 0; }
        }
        Some("bool") => quote! { self.#name = false; },
        Some("Option") => quote! { self.#name = None; },
        Some("BoundedVec") | Some("BoundedMap") => quote! { self.#name.clear(); },
        Some("AtomicU32") | Some("AtomicU64") => {
            quote! { self.#name.store(0, ::core::sync::atomic::Ordering::SeqCst); }
        }
        _ => quote! { rs_lang::StepReset::reset(&mut self.#name); },
    }
}

fn gen_wrapper_struct(cell: &CellDef) -> TokenStream {
    let name = &cell.name;
    let state_name = format_ident!("{}State", cell.name);
    let step_name = format_ident!("{}StepState", cell.name);
    let state_field_inits: Vec<TokenStream> = cell
        .state_fields
        .iter()
        .map(|f| {
            let fname = &f.name;
            quote! { #fname: Default::default(), }
        })
        .collect();
    let step_field_inits: Vec<TokenStream> = cell
        .step_state_fields
        .iter()
        .map(|f| {
            let fname = &f.name;
            quote! { #fname: Default::default(), }
        })
        .collect();
    let step_init = if cell.step_state_fields.is_empty() {
        quote! { #step_name }
    } else {
        quote! { #step_name { #(#step_field_inits)* } }
    };
    quote! {
        pub struct #name {
            state: #state_name,
            step_state: #step_name,
            __step: u64,
        }

        impl #name {
            /// Create a new cell instance with default state.
            pub fn new() -> Self {
                Self {
                    state: #state_name { #(#state_field_inits)* },
                    step_state: #step_init,
                    __step: 0,
                }
            }
        }
    }
}

fn gen_cell_trait_impl(cell: &CellDef) -> TokenStream {
    let name = &cell.name;
    let cell_name_str = cell.name.to_string();
    let version = cell.version;
    let budget = &cell.budget;
    let heartbeat = &cell.heartbeat;
    quote! {
        impl rs_lang::Cell for #name {
            const NAME: &'static str = #cell_name_str;
            const VERSION: u32 = #version;
            const BUDGET: ::core::time::Duration = #budget;
            const HEARTBEAT: ::core::time::Duration = #heartbeat;
            fn current_step(&self) -> u64 { self.__step }
            fn health_check(&self) -> rs_lang::HealthStatus {
                rs_lang::HealthStatus::Healthy
            }
            fn reset_step_state(&mut self) {
                rs_lang::StepReset::reset(&mut self.step_state);
            }
        }
    }
}

fn gen_migrate_impl(cell: &CellDef) -> TokenStream {
    let migrate = match &cell.migrate {
        Some(m) => m,
        None => return TokenStream::new(),
    };
    let state_name = format_ident!("{}State", cell.name);
    let old_type = match &migrate.from_version {
        MigrateSource::Version(n) => {
            let old_ident = format_ident!("{}StateV{}", cell.name, n);
            quote! { #old_ident }
        }
        MigrateSource::Path(path) => quote! { #path },
    };
    let field_inits: Vec<TokenStream> = migrate
        .field_mappings
        .iter()
        .map(|fm| {
            let name = &fm.name;
            let expr = &fm.expr;
            quote! { #name: #expr, }
        })
        .collect();
    quote! {
        impl rs_lang::MigrateFrom<#old_type> for #state_name {
            fn migrate(old: #old_type) -> Self {
                Self { #(#field_inits)* }
            }
        }
    }
}

fn gen_error_enum(cell: &CellDef) -> TokenStream {
    let error_name = format_ident!("{}Error", cell.name);
    let mut variants = collect_error_variants(cell);
    let has_async_deadline = cell.methods.iter().any(|m| m.deadline.is_some());
    if has_async_deadline && !variants.contains(&"Timeout".to_string()) {
        variants.push("Timeout".to_string());
    }
    if variants.is_empty() {
        return quote! {
            #[derive(Debug)]
            pub enum #error_name {}
        };
    }
    let variant_idents: Vec<Ident> = variants.iter().map(|v| format_ident!("{}", v)).collect();
    let timeout_impl = if has_async_deadline {
        quote! {
            impl From<rs_lang::Timeout> for #error_name {
                fn from(_: rs_lang::Timeout) -> Self { #error_name::Timeout }
            }
        }
    } else {
        TokenStream::new()
    };
    quote! {
        #[derive(Debug)]
        pub enum #error_name {
            #(#variant_idents,)*
        }
        #timeout_impl
    }
}

fn gen_error_aliases(cell: &CellDef) -> TokenStream {
    let error_name = format_ident!("{}Error", cell.name);
    quote! {
        /// Inside cell methods, `Error::Variant` resolves to `{CellName}Error::Variant`.
        type Error = #error_name;
        /// Inside cell methods, `Result<T>` resolves to `Result<T, {CellName}Error>`.
        #[allow(dead_code)]
        type Result<T> = core::result::Result<T, #error_name>;
    }
}

fn collect_error_variants(cell: &CellDef) -> Vec<String> {
    let mut variants = Vec::new();
    for method in &cell.methods {
        scan_for_error_variants(&method.body, &mut variants);
    }
    let mut seen = std::collections::HashSet::new();
    variants.retain(|v| seen.insert(v.clone()));
    variants
}

fn scan_for_error_variants(stream: &TokenStream, variants: &mut Vec<String>) {
    use proc_macro2::TokenTree;
    let tokens: Vec<TokenTree> = stream.clone().into_iter().collect();
    let len = tokens.len();
    for i in 0..len {
        if let TokenTree::Ident(ident) = &tokens[i] {
            if ident == "Error" && i + 3 < len {
                if let (
                    TokenTree::Punct(p1),
                    TokenTree::Punct(p2),
                    TokenTree::Ident(variant),
                ) = (&tokens[i + 1], &tokens[i + 2], &tokens[i + 3])
                {
                    if p1.as_char() == ':' && p2.as_char() == ':' {
                        variants.push(variant.to_string());
                    }
                }
            }
        }
        if let TokenTree::Group(group) = &tokens[i] {
            scan_for_error_variants(&group.stream(), variants);
        }
    }
}

fn gen_metadata_impl(cell: &CellDef) -> TokenStream {
    let name = &cell.name;
    let pub_methods: Vec<&CellMethod> = cell
        .methods
        .iter()
        .filter(|m| m.vis == MethodVis::Public)
        .collect();
    let sig_exprs: Vec<TokenStream> = pub_methods
        .iter()
        .map(|m| {
            let method_name = m.name.to_string();
            let arg_strs: Vec<String> = m
                .args
                .iter()
                .map(|a| {
                    let ty = &a.ty;
                    quote! { #ty }.to_string()
                })
                .collect();
            let ret_str = match &m.ret {
                ReturnType::Default => "()".to_string(),
                ReturnType::Type(_, ty) => quote! { #ty }.to_string(),
            };
            let deadline_expr = match &m.deadline {
                Some(d) => quote! { Some(#d) },
                None => quote! { None },
            };
            let args_array = if arg_strs.is_empty() {
                quote! { &[] }
            } else {
                let strs: Vec<TokenStream> = arg_strs.iter().map(|s| quote! { #s }).collect();
                quote! { &[#(#strs),*] }
            };
            quote! {
                rs_lang::FunctionSignature {
                    name: #method_name,
                    args: #args_array,
                    ret: #ret_str,
                    deadline: #deadline_expr,
                }
            }
        })
        .collect();
    quote! {
        impl rs_lang::CellMetadata for #name {
            fn interface() -> &'static [rs_lang::FunctionSignature] {
                &[#(#sig_exprs),*]
            }
        }
    }
}

fn gen_methods_impl(cell: &CellDef) -> TokenStream {
    let name = &cell.name;
    let error_name = format_ident!("{}Error", cell.name);
    let method_fns: Vec<TokenStream> = cell
        .methods
        .iter()
        .map(|m| gen_single_method(m, &error_name))
        .collect();
    quote! {
        impl #name {
            #(#method_fns)*
        }
    }
}

fn gen_single_method(method: &CellMethod, _error_name: &Ident) -> TokenStream {
    let vis = match method.vis {
        MethodVis::Public => quote! { pub },
        MethodVis::Private => quote! {},
    };
    let fn_name = &method.name;
    let attrs = &method.attrs;
    let self_param = match method.self_arg {
        SelfArg::Ref => quote! { &self, },
        SelfArg::RefMut => quote! { &mut self, },
        SelfArg::None => quote! {},
    };
    let params: Vec<TokenStream> = method
        .args
        .iter()
        .map(|a| {
            let name = &a.name;
            let ty = &a.ty;
            quote! { #name: #ty }
        })
        .collect();
    let ret = &method.ret;
    let body = &method.body;
    if method.is_async {
        if let Some(deadline) = &method.deadline {
            quote! {
                #(#attrs)*
                #vis fn #fn_name(#self_param #(#params),*) #ret {
                    rs_lang::runtime::with_deadline(#deadline, async move #body)
                }
            }
        } else {
            quote! {
                #(#attrs)*
                #vis async fn #fn_name(#self_param #(#params),*) #ret #body
            }
        }
    } else {
        quote! {
            #(#attrs)*
            #vis fn #fn_name(#self_param #(#params),*) #ret #body
        }
    }
}

fn gen_channel_types(cell: &CellDef) -> TokenStream {
    let name = &cell.name;
    let mut output = TokenStream::new();
    if let Some(input_ch) = &cell.input_channel {
        let alias = format_ident!("{}Input", name);
        let ty = &input_ch.ty;
        output.extend(quote! { pub type #alias = #ty; });
    }
    if let Some(output_ch) = &cell.output_channel {
        let alias = format_ident!("{}Output", name);
        let ty = &output_ch.ty;
        output.extend(quote! { pub type #alias = #ty; });
    }
    output
}

fn extract_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(type_path) => {
            let last_seg = type_path.path.segments.last()?;
            Some(last_seg.ident.to_string())
        }
        _ => None,
    }
}
