//! Module code generation.
//!
//! Takes a parsed `ModuleDef` and generates all output code:
//! state structs, wrapper, Module trait, migration, error enum,
//! metadata, methods, and channel type aliases.

use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{Ident, Result, ReturnType, Type};

use super::parse::{ModuleDef, ModuleField, ModuleMethod, MethodVis, MigrateSource, SelfArg};

pub fn generate(module: &ModuleDef) -> Result<TokenStream> {
    let state_struct = gen_state_struct(module);
    let step_state_struct = gen_step_state_struct(module);
    let wrapper_struct = gen_wrapper_struct(module);
    let module_trait_impl = gen_module_trait_impl(module);
    let migrate_impl = gen_migrate_impl(module);
    let error_enum = gen_error_enum(module);
    let error_aliases = gen_error_aliases(module);
    let metadata_impl = gen_metadata_impl(module);
    let methods_impl = gen_methods_impl(module);
    let channel_types = gen_channel_types(module);

    Ok(quote! {
        #state_struct
        #step_state_struct
        #wrapper_struct
        #module_trait_impl
        #migrate_impl
        #error_enum
        #error_aliases
        #metadata_impl
        #methods_impl
        #channel_types
    })
}

fn gen_state_struct(module: &ModuleDef) -> TokenStream {
    let state_name = format_ident!("{}State", module.name);
    let fields: Vec<TokenStream> = module
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

fn gen_step_state_struct(module: &ModuleDef) -> TokenStream {
    let step_name = format_ident!("{}StepState", module.name);
    if module.step_state_fields.is_empty() {
        return quote! {
            pub struct #step_name;
            impl rs_lang::StepReset for #step_name {
                fn reset(&mut self) {}
            }
        };
    }
    let fields: Vec<TokenStream> = module
        .step_state_fields
        .iter()
        .map(|f| {
            let name = &f.name;
            let ty = &f.ty;
            quote! { pub #name: #ty, }
        })
        .collect();
    let reset_stmts: Vec<TokenStream> = module
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

fn step_reset_stmt(field: &ModuleField) -> TokenStream {
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

fn gen_wrapper_struct(module: &ModuleDef) -> TokenStream {
    let name = &module.name;
    let state_name = format_ident!("{}State", module.name);
    let step_name = format_ident!("{}StepState", module.name);
    let state_field_inits: Vec<TokenStream> = module
        .state_fields
        .iter()
        .map(|f| {
            let fname = &f.name;
            quote! { #fname: Default::default(), }
        })
        .collect();
    let step_field_inits: Vec<TokenStream> = module
        .step_state_fields
        .iter()
        .map(|f| {
            let fname = &f.name;
            quote! { #fname: Default::default(), }
        })
        .collect();
    let step_init = if module.step_state_fields.is_empty() {
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
            /// Create a new module instance with default state.
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

fn gen_module_trait_impl(module: &ModuleDef) -> TokenStream {
    let name = &module.name;
    let module_name_str = module.name.to_string();
    let version = module.version;
    let budget = &module.budget;
    let heartbeat = &module.heartbeat;
    quote! {
        impl rs_lang::Module for #name {
            const NAME: &'static str = #module_name_str;
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

fn gen_migrate_impl(module: &ModuleDef) -> TokenStream {
    let migrate = match &module.migrate {
        Some(m) => m,
        None => return TokenStream::new(),
    };
    let state_name = format_ident!("{}State", module.name);
    let old_type = match &migrate.from_version {
        MigrateSource::Version(n) => {
            let old_ident = format_ident!("{}StateV{}", module.name, n);
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

fn gen_error_enum(module: &ModuleDef) -> TokenStream {
    let error_name = format_ident!("{}Error", module.name);
    let mut variants = collect_error_variants(module);
    let has_async_deadline = module.methods.iter().any(|m| m.deadline.is_some());
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

fn gen_error_aliases(_module: &ModuleDef) -> TokenStream {
    // The spec says `Error::Variant` resolves to `{ModuleName}Error::Variant`
    // inside module methods. This is achieved by rewriting `Error` references
    // in the method token stream to `{ModuleName}Error` during codegen.
    //
    // Token-stream rewriting happens in gen_single_method — the error enum
    // name is substituted wherever `Error` appears in a path position.
    TokenStream::new()
}

fn collect_error_variants(module: &ModuleDef) -> Vec<String> {
    let mut variants = Vec::new();
    for method in &module.methods {
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

fn gen_metadata_impl(module: &ModuleDef) -> TokenStream {
    let name = &module.name;
    let pub_methods: Vec<&ModuleMethod> = module
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
        impl rs_lang::ModuleMetadata for #name {
            fn interface() -> &'static [rs_lang::FunctionSignature] {
                const INTERFACE: &[rs_lang::FunctionSignature] = &[#(#sig_exprs),*];
                INTERFACE
            }
        }
    }
}

fn gen_methods_impl(module: &ModuleDef) -> TokenStream {
    let name = &module.name;
    let error_name = format_ident!("{}Error", module.name);
    let method_fns: Vec<TokenStream> = module
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

fn gen_single_method(method: &ModuleMethod, error_name: &Ident) -> TokenStream {
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
    let ret = rewrite_error_ident(&method.ret.to_token_stream(), error_name);
    let body = rewrite_error_ident(&method.body, error_name);
    if method.is_async {
        if let Some(deadline) = &method.deadline {
            quote! {
                #(#attrs)*
                #[allow(unknown_lints, rs_unbounded_async)] // deadline checked by the module parser
                #vis async fn #fn_name(#self_param #(#params),*) #ret {
                    rs_lang::runtime::with_deadline(#deadline, async move #body).await
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

fn gen_channel_types(module: &ModuleDef) -> TokenStream {
    let name = &module.name;
    let mut output = TokenStream::new();
    if let Some(input_ch) = &module.input_channel {
        let alias = format_ident!("{}Input", name);
        let ty = &input_ch.ty;
        output.extend(quote! { pub type #alias = #ty; });
    }
    if let Some(output_ch) = &module.output_channel {
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

/// Rewrite module-specific names in a token stream.
///
/// - `Error` → `{ModuleName}Error` (in any position)
/// - `Result<T>` → `core::result::Result<T, {ModuleName}Error>` (1-arg Result)
fn rewrite_error_ident(stream: &TokenStream, error_name: &Ident) -> TokenStream {
    use proc_macro2::TokenTree;
    let mut out = TokenStream::new();
    let tokens: Vec<TokenTree> = stream.clone().into_iter().collect();
    let len = tokens.len();
    let mut i = 0;
    while i < len {
        match &tokens[i] {
            TokenTree::Ident(ident) if ident == "Error" => {
                out.extend(quote! { #error_name });
            }
            TokenTree::Ident(ident) if ident == "Result" => {
                // Check if followed by `<T>` (angle-bracket group).
                // proc_macro2 doesn't parse `<T>` as a group, so Result<T>
                // appears as: Ident("Result") Punct('<') ... Punct('>').
                // We rewrite the entire sequence.
                if i + 1 < len {
                    if let TokenTree::Punct(p) = &tokens[i + 1] {
                        if p.as_char() == '<' {
                            // Collect tokens until matching '>'
                            let mut depth = 1;
                            let mut inner_tokens = TokenStream::new();
                            let mut j = i + 2;
                            while j < len && depth > 0 {
                                match &tokens[j] {
                                    TokenTree::Punct(p) if p.as_char() == '<' => {
                                        depth += 1;
                                        inner_tokens.extend(core::iter::once(tokens[j].clone()));
                                    }
                                    TokenTree::Punct(p) if p.as_char() == '>' => {
                                        depth -= 1;
                                        if depth > 0 {
                                            inner_tokens.extend(core::iter::once(tokens[j].clone()));
                                        }
                                    }
                                    other => {
                                        inner_tokens.extend(core::iter::once(other.clone()));
                                    }
                                }
                                j += 1;
                            }
                            let rewritten_inner = rewrite_error_ident(&inner_tokens, error_name);
                            out.extend(quote! {
                                core::result::Result<#rewritten_inner, #error_name>
                            });
                            i = j;
                            continue;
                        }
                    }
                }
                // Bare `Result` without angle brackets — pass through
                out.extend(core::iter::once(tokens[i].clone()));
            }
            TokenTree::Group(group) => {
                let rewritten = rewrite_error_ident(&group.stream(), error_name);
                let mut new_group =
                    proc_macro2::Group::new(group.delimiter(), rewritten);
                new_group.set_span(group.span());
                out.extend(core::iter::once(TokenTree::Group(new_group)));
            }
            other => {
                out.extend(core::iter::once(other.clone()));
            }
        }
        i += 1;
    }
    out
}
