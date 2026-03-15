//! Cell DSL parser.
//!
//! Parses the token stream inside `cell! { ... }` into a `CellDef` structure.
//! The DSL supports:
//! - name: Ident
//! - version: u32
//! - budget: Duration expr
//! - heartbeat: Duration expr
//! - state { field: Type, ... }
//! - step_state { field: Type, ... }
//! - input: BoundedChannel<T, N>
//! - output: BoundedChannel<T, N>
//! - pub fn / fn methods (sync and async(dur))
//! - migrate from vN { field: expr, ... }

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    braced, parenthesized, Attribute, Block, Error, Expr, Ident, Result, ReturnType, Token, Type,
};
use syn::parse::{Parse, ParseStream};

// ---------------------------------------------------------------------------
// AST types
// ---------------------------------------------------------------------------

pub struct CellDef {
    pub name: Ident,
    pub version: u32,
    pub budget: Expr,
    pub heartbeat: Expr,
    pub state_fields: Vec<CellField>,
    pub step_state_fields: Vec<CellField>,
    pub methods: Vec<CellMethod>,
    pub migrate: Option<MigrateDef>,
    pub input_channel: Option<ChannelDef>,
    pub output_channel: Option<ChannelDef>,
}

pub struct CellField {
    pub name: Ident,
    pub ty: Type,
}

pub struct CellMethod {
    pub vis: MethodVis,
    pub is_async: bool,
    pub deadline: Option<Expr>,
    pub name: Ident,
    pub self_arg: SelfArg,
    pub args: Vec<MethodArg>,
    pub ret: ReturnType,
    pub body: TokenStream,
    pub attrs: Vec<Attribute>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum MethodVis {
    Public,
    Private,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SelfArg {
    Ref,     // &self
    RefMut,  // &mut self
    None,    // no self (static method)
}

pub struct MethodArg {
    pub name: Ident,
    pub ty: Type,
}

pub struct MigrateDef {
    pub from_version: MigrateSource,
    pub field_mappings: Vec<FieldMapping>,
}

pub enum MigrateSource {
    /// `migrate from v3` — resolves to `{CellName}StateV3`
    Version(u32),
    /// `migrate from my_module::OldState` — literal path
    Path(syn::Path),
}

pub struct FieldMapping {
    pub name: Ident,
    pub expr: Expr,
}

pub struct ChannelDef {
    pub ty: Type,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub fn parse_cell(input: TokenStream) -> Result<CellDef> {
    syn::parse2(input)
}

impl Parse for CellDef {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut name: Option<Ident> = None;
        let mut version: Option<u32> = None;
        let mut budget: Option<Expr> = None;
        let mut heartbeat: Option<Expr> = None;
        let mut state_fields: Vec<CellField> = Vec::new();
        let mut step_state_fields: Vec<CellField> = Vec::new();
        let mut methods: Vec<CellMethod> = Vec::new();
        let mut migrate: Option<MigrateDef> = None;
        let mut input_channel: Option<ChannelDef> = None;
        let mut output_channel: Option<ChannelDef> = None;

        while !input.is_empty() {
            // Eat any leading attributes.
            let attrs: Vec<Attribute> = input.call(Attribute::parse_outer)?;

            if input.peek(Token![pub]) || input.peek(Token![fn]) {
                // Method declaration.
                let method = parse_method(input, attrs)?;
                methods.push(method);
                // Optional trailing comma.
                let _ = input.parse::<Option<Token![,]>>();
                continue;
            }

            // Check for `async` keyword (method with async).
            if input.peek(Token![async]) {
                let method = parse_method(input, attrs)?;
                methods.push(method);
                let _ = input.parse::<Option<Token![,]>>();
                continue;
            }

            // Keyword fields.
            let ident: Ident = input.parse()?;
            let ident_str = ident.to_string();

            match ident_str.as_str() {
                "name" => {
                    input.parse::<Token![:]>()?;
                    name = Some(input.parse()?);
                    input.parse::<Token![,]>()?;
                }
                "version" => {
                    input.parse::<Token![:]>()?;
                    let lit: syn::LitInt = input.parse()?;
                    version = Some(lit.base10_parse()?);
                    input.parse::<Token![,]>()?;
                }
                "budget" => {
                    input.parse::<Token![:]>()?;
                    budget = Some(input.parse()?);
                    input.parse::<Token![,]>()?;
                }
                "heartbeat" => {
                    input.parse::<Token![:]>()?;
                    heartbeat = Some(input.parse()?);
                    input.parse::<Token![,]>()?;
                }
                "state" => {
                    let content;
                    braced!(content in input);
                    state_fields = parse_fields(&content)?;
                    let _ = input.parse::<Option<Token![,]>>();
                }
                "step_state" => {
                    let content;
                    braced!(content in input);
                    step_state_fields = parse_fields(&content)?;
                    let _ = input.parse::<Option<Token![,]>>();
                }
                "input" => {
                    input.parse::<Token![:]>()?;
                    let ty: Type = input.parse()?;
                    input_channel = Some(ChannelDef { ty });
                    input.parse::<Token![,]>()?;
                }
                "output" => {
                    input.parse::<Token![:]>()?;
                    let ty: Type = input.parse()?;
                    output_channel = Some(ChannelDef { ty });
                    input.parse::<Token![,]>()?;
                }
                "migrate" => {
                    migrate = Some(parse_migrate(input)?);
                    let _ = input.parse::<Option<Token![,]>>();
                }
                other => {
                    return Err(Error::new(
                        ident.span(),
                        format!("unexpected keyword `{}` in cell! declaration", other),
                    ));
                }
            }
        }

        Ok(CellDef {
            name: name.ok_or_else(|| Error::new(Span::call_site(), "missing `name` in cell!"))?,
            version: version
                .ok_or_else(|| Error::new(Span::call_site(), "missing `version` in cell!"))?,
            budget: budget
                .ok_or_else(|| Error::new(Span::call_site(), "missing `budget` in cell!"))?,
            heartbeat: heartbeat
                .ok_or_else(|| Error::new(Span::call_site(), "missing `heartbeat` in cell!"))?,
            state_fields,
            step_state_fields,
            methods,
            migrate,
            input_channel,
            output_channel,
        })
    }
}

fn parse_fields(input: ParseStream) -> Result<Vec<CellField>> {
    let mut fields = Vec::new();
    while !input.is_empty() {
        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty: Type = input.parse()?;
        fields.push(CellField { name, ty });
        if input.is_empty() {
            break;
        }
        input.parse::<Token![,]>()?;
    }
    Ok(fields)
}

fn parse_method(input: ParseStream, attrs: Vec<Attribute>) -> Result<CellMethod> {
    // Parse visibility.
    let vis = if input.peek(Token![pub]) {
        input.parse::<Token![pub]>()?;
        MethodVis::Public
    } else {
        MethodVis::Private
    };

    // Parse async and optional deadline: async(Duration::from_millis(100))
    let mut is_async = false;
    let mut deadline: Option<Expr> = None;

    if input.peek(Token![async]) {
        input.parse::<Token![async]>()?;
        is_async = true;

        // Check for (deadline_expr)
        if input.peek(syn::token::Paren) {
            let content;
            parenthesized!(content in input);
            deadline = Some(content.parse()?);
        }
    }

    // Parse fn keyword.
    input.parse::<Token![fn]>()?;

    // Parse function name.
    let name: Ident = input.parse()?;

    // Parse arguments.
    let arg_content;
    parenthesized!(arg_content in input);

    let mut self_arg = SelfArg::None;
    let mut args = Vec::new();

    if !arg_content.is_empty() {
        // Check for &self or &mut self.
        if arg_content.peek(Token![&]) {
            let fork = arg_content.fork();
            fork.parse::<Token![&]>()?;
            if fork.peek(Token![mut]) && fork.peek2(Token![self]) {
                // &mut self
                arg_content.parse::<Token![&]>()?;
                arg_content.parse::<Token![mut]>()?;
                arg_content.parse::<Token![self]>()?;
                self_arg = SelfArg::RefMut;
            } else if fork.peek(Token![self]) {
                // &self
                arg_content.parse::<Token![&]>()?;
                arg_content.parse::<Token![self]>()?;
                self_arg = SelfArg::Ref;
            }

            // Skip comma after self.
            if !arg_content.is_empty() {
                arg_content.parse::<Token![,]>()?;
            }
        }

        // Parse remaining arguments.
        while !arg_content.is_empty() {
            let arg_name: Ident = arg_content.parse()?;
            arg_content.parse::<Token![:]>()?;
            let arg_ty: Type = arg_content.parse()?;
            args.push(MethodArg {
                name: arg_name,
                ty: arg_ty,
            });
            if arg_content.is_empty() {
                break;
            }
            arg_content.parse::<Token![,]>()?;
        }
    }

    // Parse return type.
    let ret: ReturnType = input.parse()?;

    // Parse body.
    let body_block: Block = input.parse()?;
    let body = quote! { #body_block };

    Ok(CellMethod {
        vis,
        is_async,
        deadline,
        name,
        self_arg,
        args,
        ret,
        body,
        attrs,
    })
}

fn parse_migrate(input: ParseStream) -> Result<MigrateDef> {
    // Parse: migrate from vN { field: expr, ... }
    // or:    migrate from path::To::Type { field: expr, ... }
    let from_kw: Ident = input.parse()?;
    if from_kw != "from" {
        return Err(Error::new(from_kw.span(), "expected `from` after `migrate`"));
    }

    let source = parse_migrate_source(input)?;

    let content;
    braced!(content in input);

    let mut field_mappings = Vec::new();
    while !content.is_empty() {
        let name: Ident = content.parse()?;
        content.parse::<Token![:]>()?;
        let expr: Expr = content.parse()?;
        field_mappings.push(FieldMapping { name, expr });
        if content.is_empty() {
            break;
        }
        content.parse::<Token![,]>()?;
    }

    Ok(MigrateDef {
        from_version: source,
        field_mappings,
    })
}

fn parse_migrate_source(input: ParseStream) -> Result<MigrateSource> {
    // Try to parse `vN` where N is a number.
    let fork = input.fork();
    if let Ok(ident) = fork.parse::<Ident>() {
        let s = ident.to_string();
        if s.starts_with('v') {
            if let Ok(n) = s[1..].parse::<u32>() {
                // Advance the real input.
                input.parse::<Ident>()?;
                return Ok(MigrateSource::Version(n));
            }
        }
    }

    // Otherwise, parse as a full path.
    let path: syn::Path = input.parse()?;
    Ok(MigrateSource::Path(path))
}
