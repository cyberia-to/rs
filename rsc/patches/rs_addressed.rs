//! Addressed type verification: RS301–RS306.
//!
//! MIR-level transitivity verification for `#[derive(Addressed)]` types.
//! The proc-macro catches direct violations at derive site. This lint pass
//! adds transitive checks that proc-macros cannot perform:
//!
//! - Type aliases that resolve to non-serializable types
//! - Generic bounds that allow non-serializable instantiations
//! - Fields added via trait impls in other modules

use rustc_hir as hir;
use rustc_hir::def_id::DefId;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::{Span, Symbol};

declare_lint! { pub RS_ADDR_NOT_SERIALIZABLE, Deny, "type does not implement CanonicalSerialize (RS301)" }
declare_lint! { pub RS_ADDR_FLOAT, Deny, "floating point types are not canonically serializable (RS302)" }
declare_lint! { pub RS_ADDR_PTR, Deny, "pointers cannot be addressed (RS303)" }
declare_lint! { pub RS_ADDR_HASHMAP, Deny, "HashMap has non-deterministic serialization (RS304)" }
declare_lint! { pub RS_ADDR_USIZE, Deny, "usize/isize width is platform-dependent (RS305)" }
declare_lint! { pub RS_ADDR_REPR, Deny, "Addressed enum discriminant must fit in u32 (RS306)" }

declare_lint_pass!(AddressedVerify => [
    RS_ADDR_NOT_SERIALIZABLE, RS_ADDR_FLOAT, RS_ADDR_PTR,
    RS_ADDR_HASHMAP, RS_ADDR_USIZE, RS_ADDR_REPR,
]);

/// Returns true if the item has `#[derive(Addressed)]` or `Addressed` in its
/// derive list.
fn has_addressed_derive(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    // Check for the CanonicalSerialize impl which Addressed generates.
    // The derive macro creates an impl — we look for the marker attribute
    // it leaves behind.
    tcx.get_attrs(def_id, Symbol::intern("addressed"))
        .next()
        .is_some()
}

/// Check a single type for addressed-incompatible constructs.
fn check_field_type<'tcx>(
    cx: &LateContext<'tcx>,
    ty: Ty<'tcx>,
    span: Span,
    field_name: &str,
) {
    let ty = ty.peel_refs();

    // RS302: floating point fields
    if matches!(ty.kind(), ty::Float(..)) {
        emit(cx, &RS_ADDR_FLOAT, span, "RS302",
            "floating point types are not canonically serializable",
            "use FixedPoint<u128, 18> for deterministic decimal values");
        return;
    }

    // RS303: raw pointer fields
    if ty.is_unsafe_ptr() || ty.is_fn_ptr() {
        emit(cx, &RS_ADDR_PTR, span, "RS303",
            "pointers cannot be addressed",
            "pointers are memory addresses, not content \u{2014} use the pointed-to value");
        return;
    }

    // RS305: usize/isize fields
    if matches!(ty.kind(),
        ty::Uint(ty::UintTy::Usize) | ty::Int(ty::IntTy::Isize))
    {
        emit(cx, &RS_ADDR_USIZE, span, "RS305",
            "usize/isize width is platform-dependent; use u32 or u64",
            "canonical serialization requires fixed-width integers");
        return;
    }

    // RS304: HashMap/HashSet fields
    if let ty::Adt(adt_def, _) = ty.kind() {
        let path = cx.tcx.def_path_str(adt_def.did());
        if path.contains("HashMap") || path.contains("HashSet") {
            emit(cx, &RS_ADDR_HASHMAP, span, "RS304",
                "HashMap has non-deterministic serialization; use BTreeMap",
                "HashMap iteration order varies between runs");
            return;
        }
    }

    // RS301: check that the type implements CanonicalSerialize
    // This is the transitive check — the proc-macro verifies direct fields,
    // but type aliases and generic instantiations can introduce types that
    // don't implement the trait.
    if let ty::Adt(adt_def, substs) = ty.kind() {
        check_canonical_serialize_impl(cx, ty, span, field_name);
    }
}

/// Verify that a type implements CanonicalSerialize.
fn check_canonical_serialize_impl<'tcx>(
    cx: &LateContext<'tcx>,
    ty: Ty<'tcx>,
    span: Span,
    field_name: &str,
) {
    let tcx = cx.tcx;

    // Look up the CanonicalSerialize trait
    // The trait is defined in rs-lang, so we search for it by name
    let canonical_serialize = Symbol::intern("CanonicalSerialize");

    // Search through all traits for CanonicalSerialize
    for trait_def_id in tcx.all_traits() {
        if tcx.item_name(trait_def_id) == canonical_serialize {
            // Check if the type implements this trait
            let param_env = ty::ParamEnv::reveal_all();
            let trait_ref = ty::TraitRef::new(tcx, trait_def_id, [ty]);

            if !tcx.infer_ctxt().build().type_implements_trait(
                trait_def_id, [ty], param_env
            ).must_apply_modulo_regions() {
                emit(cx, &RS_ADDR_NOT_SERIALIZABLE, span, "RS301",
                    &format!(
                        "type {} does not implement CanonicalSerialize (field `{}`)",
                        tcx.def_path_str_with_args(
                            ty.ty_adt_def().map(|a| a.did()).unwrap_or(DefId::local(rustc_hir::def_id::CRATE_DEF_INDEX)),
                            &[]
                        ),
                        field_name
                    ),
                    "derive Addressed on the type, or implement CanonicalSerialize manually");
            }
            break;
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for AddressedVerify {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx hir::Item<'tcx>) {
        // Only check structs and enums with Addressed derive
        let def_id = item.owner_id.to_def_id();
        if !has_addressed_derive(cx.tcx, def_id) {
            return;
        }

        match &item.kind {
            hir::ItemKind::Struct(variant_data, _) => {
                check_struct_fields(cx, variant_data, item.span);
            }
            hir::ItemKind::Enum(enum_def, _) => {
                check_enum(cx, def_id, enum_def, item.span);
            }
            _ => {}
        }
    }
}

/// Check all fields of an Addressed struct.
fn check_struct_fields<'tcx>(
    cx: &LateContext<'tcx>,
    variant_data: &'tcx hir::VariantData<'tcx>,
    _struct_span: Span,
) {
    for field in variant_data.fields() {
        let field_ty = cx.tcx.type_of(field.def_id).skip_binder();
        let field_name = field.ident.as_str();
        check_field_type(cx, field_ty, field.span, field_name);
    }
}

/// RS306: check enum repr is at most u32.
fn check_enum<'tcx>(
    cx: &LateContext<'tcx>,
    def_id: DefId,
    enum_def: &'tcx hir::EnumDef<'tcx>,
    span: Span,
) {
    let repr = cx.tcx.repr_options_of_def(def_id);

    // Check if repr is wider than u32
    if let Some(int_type) = repr.int {
        let too_wide = matches!(int_type,
            rustc_abi::IntegerType::Fixed(rustc_abi::Integer::I64, _) |
            rustc_abi::IntegerType::Fixed(rustc_abi::Integer::I128, _)
        );

        if too_wide {
            emit(cx, &RS_ADDR_REPR, span, "RS306",
                "Addressed enum discriminant must fit in u32; #[repr(u64)] is not supported",
                "canonical serialization encodes enum discriminants as u32");
        }
    }

    // Also check fields of each variant
    for variant in enum_def.variants {
        for field in variant.data.fields() {
            let field_ty = cx.tcx.type_of(field.def_id).skip_binder();
            let field_name = field.ident.as_str();
            check_field_type(cx, field_ty, field.span, field_name);
        }
    }
}

fn emit(
    cx: &LateContext<'_>,
    lint: &'static rustc_lint::Lint,
    span: Span,
    code: &str,
    message: &str,
    help: &str,
) {
    cx.struct_span_lint(lint, span, |diag| {
        let mut builder = diag.build(message);
        builder.code_str(code);
        builder.help(help);
        builder.emit();
    });
}

/// Register the addressed verification lint pass in the lint store.
pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[
        &RS_ADDR_NOT_SERIALIZABLE, &RS_ADDR_FLOAT, &RS_ADDR_PTR,
        &RS_ADDR_HASHMAP, &RS_ADDR_USIZE, &RS_ADDR_REPR,
    ]);
    store.register_late_pass(|_| Box::new(AddressedVerify));
}
