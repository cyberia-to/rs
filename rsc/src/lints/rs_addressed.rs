//! Addressed type verification: RS302-RS305.
//!
//! Checks fields of types marked with `#[addressed]` attribute.
//! RS301 (CanonicalSerialize) and RS306 (repr) require deeper analysis
//! deferred to a future pass.

use rustc_hir as hir;
use rustc_hir::def_id::DefId;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::{Span, Symbol};

declare_lint! { pub RS_ADDR_FLOAT, Deny, "float in Addressed type (RS302)" }
declare_lint! { pub RS_ADDR_PTR, Deny, "pointer in Addressed type (RS303)" }
declare_lint! { pub RS_ADDR_HASHMAP, Deny, "HashMap in Addressed type (RS304)" }
declare_lint! { pub RS_ADDR_USIZE, Deny, "usize/isize in Addressed type (RS305)" }

declare_lint_pass!(AddressedVerify => [
    RS_ADDR_FLOAT, RS_ADDR_PTR, RS_ADDR_HASHMAP, RS_ADDR_USIZE,
]);

fn has_addressed_derive(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    tcx.get_attrs(def_id, Symbol::intern("addressed"))
        .next()
        .is_some()
}

fn check_field_type(cx: &LateContext<'_>, ty: Ty<'_>, span: Span) {
    let ty = ty.peel_refs();

    if matches!(ty.kind(), ty::Float(..)) {
        cx.span_lint(RS_ADDR_FLOAT, span, |diag| {
            diag.primary_message("floating point in Addressed type (RS302)");
            diag.help("use FixedPoint<u128, 18>");
        });
        return;
    }
    if ty.is_raw_ptr() || ty.is_fn_ptr() {
        cx.span_lint(RS_ADDR_PTR, span, |diag| {
            diag.primary_message("pointer in Addressed type (RS303)");
            diag.help("use the pointed-to value directly");
        });
        return;
    }
    if matches!(ty.kind(), ty::Uint(ty::UintTy::Usize) | ty::Int(ty::IntTy::Isize)) {
        cx.span_lint(RS_ADDR_USIZE, span, |diag| {
            diag.primary_message("usize/isize in Addressed type (RS305)");
            diag.help("use u32 or u64 for canonical serialization");
        });
        return;
    }
    if let ty::Adt(adt_def, _) = ty.kind() {
        let path = cx.tcx.def_path_str(adt_def.did());
        if path.contains("HashMap") || path.contains("HashSet") {
            cx.span_lint(RS_ADDR_HASHMAP, span, |diag| {
                diag.primary_message("HashMap in Addressed type (RS304)");
                diag.help("use BTreeMap or BoundedMap");
            });
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for AddressedVerify {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx hir::Item<'tcx>) {
        let def_id = item.owner_id.to_def_id();
        if !has_addressed_derive(cx.tcx, def_id) {
            return;
        }

        // Check all fields via the type system instead of HIR pattern matching
        let ty = cx.tcx.type_of(item.owner_id).skip_binder();
        if let ty::Adt(adt_def, args) = ty.kind() {
            for variant in adt_def.variants() {
                for field in &variant.fields {
                    let field_ty = field.ty(cx.tcx, args);
                    check_field_type(cx, field_ty, item.span);
                }
            }
        }
    }
}

pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[
        &RS_ADDR_FLOAT, &RS_ADDR_PTR, &RS_ADDR_HASHMAP, &RS_ADDR_USIZE,
    ]);
    store.register_late_pass(|_| Box::new(AddressedVerify));
}
