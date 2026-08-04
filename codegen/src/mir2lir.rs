//! MIR → LIR lowering. Both IRs are flat 3-address; this is a near-1:1 mapping.

use std::collections::HashMap;

use rustc_middle::mir;
use rustc_middle::mir::ConstValue;
use rustc_middle::mir::interpret::{GlobalAlloc, Scalar};
use rustc_middle::ty::{self, Instance, TyCtxt, TyKind};
use rustc_middle::ty::adjustment::PointerCoercion;

use crate::lir::{Label, LIROp, Reg};

pub struct StaticData {
    pub name:     String,
    pub bytes:    Vec<u8>,
    pub writable: bool,
}

/// A pointer-sized relocation within a static data block:
/// at `byte_offset` within the static named `static_name`,
/// store the absolute VM address of `fn_symbol`.
pub struct StaticReloc {
    pub static_name: String,
    pub byte_offset: usize,
    pub fn_symbol:   String,
}

/// Whether a MIR place lives in a register or behind a memory pointer.
enum PlaceLoc {
    Reg(Reg),   // value is directly in the virtual register
    Mem(Reg),   // value lives at the 64-bit address in this register
}

pub struct MirToLir<'tcx> {
    tcx:          TyCtxt<'tcx>,
    instance:     Instance<'tcx>,  // current function being lowered, for subst normalization
    next_vreg:    u32,
    ops:          Vec<LIROp>,
    locals:       HashMap<mir::Local, Reg>,
    /// Locals whose register holds a *pointer* to their value (memory-backed).
    /// When lower_place encounters such a local with no projection, it returns PlaceLoc::Mem.
    mem_locals:   std::collections::HashSet<mir::Local>,
    statics:      Vec<StaticData>,
    static_relocs: Vec<StaticReloc>,
    tls_vars:     HashMap<String, u64>,  // storage_sym → byte size
    agg_counter:  u32,
}

impl<'tcx> MirToLir<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> Self {
        Self {
            tcx, instance, next_vreg: 0, ops: Vec::new(), locals: HashMap::new(),
            mem_locals: std::collections::HashSet::new(),
            statics: Vec::new(), static_relocs: Vec::new(),
            tls_vars: HashMap::new(), agg_counter: 0,
        }
    }

    pub fn take_statics(&mut self) -> Vec<StaticData> { std::mem::take(&mut self.statics) }
    pub fn take_static_relocs(&mut self) -> Vec<StaticReloc> { std::mem::take(&mut self.static_relocs) }
    pub fn take_tls_vars(&mut self) -> HashMap<String, u64> { std::mem::take(&mut self.tls_vars) }

    fn alloc_reg(&mut self) -> Reg {
        let r = Reg(self.next_vreg);
        self.next_vreg += 1;
        r
    }

    fn local_reg(&mut self, local: mir::Local) -> Reg {
        if let Some(&r) = self.locals.get(&local) { return r; }
        let r = self.alloc_reg();
        self.locals.insert(local, r);
        r
    }

    pub fn lower(
        &mut self,
        body: &mir::Body<'tcx>,
        instance: Instance<'tcx>,
    ) -> Result<Vec<LIROp>, String> {
        self.instance = instance;
        self.ops.clear();
        self.locals.clear();
        self.mem_locals.clear();
        self.next_vreg = 0;

        let fn_name = self.tcx.symbol_name(instance).name.to_string();
        self.ops.push(LIROp::FnStart(fn_name));

        // ABI: local 0 = return place, 1..=arg_count = args → x0..xN
        let arg_count = body.arg_count;
        // Track which arm64 register slots have been consumed by arguments.
        // ARM64 AAPCS64: each arg takes one register slot per 8-byte word.
        // Composite types (structs, enums) > 8 bytes are passed by pointer if > 16 bytes,
        // or split across consecutive registers if ≤ 16 bytes.
        // For simplicity we use a per-arg reg slot counter.

        // Pre-compute total register slots consumed by all args, so we can
        // allocate scratch registers above the arg range (avoids clobbering).
        let mut total_arg_slots = 0u32;
        for i in 1..=arg_count {
            let local = mir::Local::from_usize(i);
            let arg_ty = body.local_decls[local].ty;
            let arg_size = self.type_size(arg_ty);
            total_arg_slots += if arg_size <= 8 { 1 } else { ((arg_size + 7) / 8) as u32 };
        }
        // Start virtual register allocator ABOVE all arg registers.
        self.next_vreg = total_arg_slots.max(15);

        let mut reg_slot = 0u32;
        for i in 1..=arg_count {
            let local = mir::Local::from_usize(i);
            let arg_ty = body.local_decls[local].ty;
            let arg_size = self.type_size(arg_ty);
            let slots = if arg_size <= 8 { 1u32 } else { ((arg_size + 7) / 8) as u32 };
            // If the argument is a composite type occupying >1 register slot, spill it
            // to a writable memory location so Field projections work correctly.
            if slots > 1 && self.is_composite_type(arg_ty) {
                // Allocate a BSS static for the spill slot.
                let spill_sym = format!("__arg_spill_{}_{}", self.agg_counter, i);
                self.agg_counter += 1;
                self.statics.push(StaticData {
                    name: spill_sym.clone(),
                    bytes: vec![0u8; arg_size as usize],
                    writable: true,
                });
                // Allocate scratch register above the arg range (safe: next_vreg >= total_arg_slots).
                let ptr_reg = self.alloc_reg();
                self.ops.push(LIROp::LoadAddr { dst: ptr_reg, symbol: spill_sym });
                // Store each argument register slot into the spill area.
                for slot in 0..slots {
                    let src_reg = Reg(reg_slot + slot);
                    let off = (slot * 8) as i32;
                    self.ops.push(LIROp::Store { src: src_reg, base: ptr_reg, offset: off });
                }
                // Map the local to the pointer to the spill area.
                self.locals.insert(local, ptr_reg);
                // Mark as memory-backed so lower_place treats it as Mem.
                self.mem_locals.insert(local);
            } else {
                self.locals.insert(local, Reg(reg_slot));
            }
            reg_slot += slots;
        }
        self.locals.insert(mir::Local::from_usize(0), Reg(0));

        for (bb_idx, bb_data) in body.basic_blocks.iter_enumerated() {
            self.ops.push(LIROp::LabelDef(Label::new(format!("bb{}", bb_idx.index()))));
            for stmt in &bb_data.statements {
                self.lower_statement(stmt, body)?;
            }
            self.lower_terminator(bb_data.terminator(), body)?;
        }

        self.ops.push(LIROp::FnEnd);
        Ok(std::mem::take(&mut self.ops))
    }

    // ── Statement ──────────────────────────────────────────────────────────

    fn lower_statement(
        &mut self,
        stmt: &mir::Statement<'tcx>,
        body: &mir::Body<'tcx>,
    ) -> Result<(), String> {
        match &stmt.kind {
            mir::StatementKind::Assign(box (place, rvalue)) => {
                // Special case: composite aggregate (enum/struct with size > 8 bytes) assigned
                // to a simple local place (no projection).  Materialize to a BSS static and
                // mark the local as memory-backed, so downstream code can find its fields.
                if place.projection.is_empty()
                    && !self.mem_locals.contains(&place.local)
                {
                    let place_ty = place.ty(&body.local_decls, self.tcx).ty;
                    if self.type_size(place_ty) > 8 && self.is_composite_type(place_ty) {
                        let local = place.local;
                        let ptr_reg = self.lower_composite_aggregate(rvalue, place_ty, body)?;
                        self.locals.insert(local, ptr_reg);
                        self.mem_locals.insert(local);
                        return Ok(());
                    }
                }
                match self.lower_place(place, body)? {
                    PlaceLoc::Reg(dst) => self.lower_rvalue_into(dst, rvalue, body)?,
                    PlaceLoc::Mem(ptr) => {
                        let tmp = self.alloc_reg();
                        self.lower_rvalue_into(tmp, rvalue, body)?;
                        self.ops.push(LIROp::Store { src: tmp, base: ptr, offset: 0 });
                    }
                }
            }
            mir::StatementKind::StorageLive(_)
            | mir::StatementKind::StorageDead(_)
            | mir::StatementKind::Nop => {}
            mir::StatementKind::SetDiscriminant { place, variant_index } => {
                // Write the enum discriminant (tag value) at offset 0 of the enum.
                let tag = variant_index.as_u32() as u64;
                let ptr = match self.lower_place(place, body)? {
                    PlaceLoc::Reg(r) => r,
                    PlaceLoc::Mem(ptr) => ptr,
                };
                let tmp = self.alloc_reg();
                self.ops.push(LIROp::LoadImm(tmp, tag));
                self.ops.push(LIROp::Store { src: tmp, base: ptr, offset: 0 });
            }
            _ => {
                self.ops.push(LIROp::Comment(format!("stmt: {:?}", stmt.kind)));
            }
        }
        Ok(())
    }

    // ── Composite aggregate materialization ────────────────────────────────

    /// Materialize a composite rvalue (enum or struct larger than 8 bytes) into a
    /// freshly allocated writable BSS static.  Returns the register holding the
    /// pointer to that static.  The caller is responsible for marking the target
    /// local as a mem_local.
    fn lower_composite_aggregate(
        &mut self,
        rvalue: &mir::Rvalue<'tcx>,
        place_ty: ty::Ty<'tcx>,
        body: &mir::Body<'tcx>,
    ) -> Result<Reg, String> {
        let size = self.type_size(place_ty) as usize;
        let sym = format!("__cagg_{}", self.agg_counter);
        self.agg_counter += 1;
        self.statics.push(StaticData {
            name: sym.clone(),
            bytes: vec![0u8; size],
            writable: true,
        });
        let ptr = self.alloc_reg();
        self.ops.push(LIROp::LoadAddr { dst: ptr, symbol: sym });

        match rvalue {
            mir::Rvalue::Aggregate(kind, fields) => {
                use rustc_abi::Variants;
                // Determine variant index and write the discriminant if it's an enum.
                let variant_idx: Option<rustc_abi::VariantIdx> = match kind.as_ref() {
                    mir::AggregateKind::Adt(_, variant_idx, ..) => {
                        // Write discriminant at offset 0 (4-byte i32 tag).
                        let disc_val = variant_idx.as_u32() as u64;
                        let tmp = self.alloc_reg();
                        self.ops.push(LIROp::LoadImm(tmp, disc_val));
                        self.ops.push(LIROp::StoreSize { src: tmp, base: ptr, offset: 0, size: 4 });
                        Some(*variant_idx)
                    }
                    _ => None,
                };

                // Write each field at its correct byte offset.
                for (idx, _) in fields.iter_enumerated() {
                    let field_operand = &fields[idx];
                    let field_ty = field_operand.ty(&body.local_decls, self.tcx);
                    let field_offset = self.field_byte_offset_with_variant(
                        place_ty, idx.as_usize(), variant_idx,
                    ) as i32;
                    let src = self.lower_operand(field_operand, body)?;
                    let fsz = self.type_size(field_ty) as u8;
                    if fsz > 0 && fsz < 8 {
                        self.ops.push(LIROp::StoreSize { src, base: ptr, offset: field_offset, size: fsz });
                    } else {
                        self.ops.push(LIROp::Store { src, base: ptr, offset: field_offset });
                    }
                }
            }
            // For Use/Copy of another composite local, copy its bytes register-by-register.
            mir::Rvalue::Use(operand) => {
                let src_ptr = self.lower_operand(operand, body)?;
                // Copy each 8-byte slot.
                let slots = (size + 7) / 8;
                for slot in 0..slots {
                    let tmp = self.alloc_reg();
                    let off = (slot * 8) as i32;
                    self.ops.push(LIROp::Load { dst: tmp, base: src_ptr, offset: off });
                    self.ops.push(LIROp::Store { src: tmp, base: ptr, offset: off });
                }
            }
            // Fallback: use standard rvalue lowering which puts a value or pointer in tmp,
            // then store it as the first slot.
            _ => {
                let tmp = self.alloc_reg();
                self.lower_rvalue_into(tmp, rvalue, body)?;
                self.ops.push(LIROp::Store { src: tmp, base: ptr, offset: 0 });
            }
        }
        Ok(ptr)
    }

    // ── Place lowering ─────────────────────────────────────────────────────

    fn lower_place(
        &mut self,
        place: &mir::Place<'tcx>,
        body: &mir::Body<'tcx>,
    ) -> Result<PlaceLoc, String> {
        if place.projection.is_empty() {
            if self.mem_locals.contains(&place.local) {
                return Ok(PlaceLoc::Mem(self.local_reg(place.local)));
            }
            return Ok(PlaceLoc::Reg(self.local_reg(place.local)));
        }

        let mut cur_reg = self.local_reg(place.local);
        let mut cur_ty  = body.local_decls[place.local].ty;
        // If the local is memory-backed (e.g. spilled composite arg), start in Mem mode.
        let mut is_mem  = self.mem_locals.contains(&place.local);
        // When we enter a Downcast, track which variant index we're in so that
        // Field projections can use the variant's own layout for offset calculation.
        let mut downcast_variant: Option<(ty::Ty<'tcx>, rustc_abi::VariantIdx)> = None;

        for proj in place.projection.iter() {
            match proj {
                mir::PlaceElem::Deref => {
                    // cur_reg holds a pointer; the pointee is at that address.
                    cur_ty = match cur_ty.kind() {
                        TyKind::Ref(_, inner, _) => *inner,
                        TyKind::RawPtr(inner, _) => *inner,
                        TyKind::Adt(..) => cur_ty, // box — treat as pointer
                        _ => cur_ty,
                    };
                    downcast_variant = None;
                    is_mem = true;
                    // No instruction: cur_reg already IS the address.
                }
                mir::PlaceElem::Field(field_idx, field_ty) => {
                    let offset = self.field_byte_offset_with_variant(
                        cur_ty, field_idx.as_usize(), downcast_variant.map(|(_, v)| v));
                    downcast_variant = None;
                    if !is_mem {
                        // Base is a register-valued aggregate (e.g., CheckedBinaryOp result).
                        // Extract the field by shifting/masking rather than memory access.
                        let result = self.alloc_reg();
                        if offset == 0 {
                            let fbits = self.type_bits(field_ty);
                            if fbits < 64 {
                                self.ops.push(LIROp::ZeroExt { dst: result, src: cur_reg, from_bits: fbits as u8 });
                            } else {
                                self.ops.push(LIROp::Move(result, cur_reg));
                            }
                        } else {
                            let shift = offset * 8;
                            let shift_reg = self.alloc_reg();
                            self.ops.push(LIROp::LoadImm(shift_reg, shift));
                            self.ops.push(LIROp::Shr(result, cur_reg, shift_reg));
                            let fbits = self.type_bits(field_ty);
                            if fbits < 64 {
                                self.ops.push(LIROp::ZeroExt { dst: result, src: result, from_bits: fbits as u8 });
                            }
                        }
                        cur_reg = result;
                        cur_ty = field_ty;
                        // Stay non-mem: result is in register, not memory.
                    } else {
                        if offset != 0 {
                            let off = self.alloc_reg();
                            let addr = self.alloc_reg();
                            self.ops.push(LIROp::LoadImm(off, offset));
                            self.ops.push(LIROp::Add(addr, cur_reg, off));
                            cur_reg = addr;
                        }
                        cur_ty = field_ty;
                        is_mem = true;
                    }
                }
                mir::PlaceElem::Index(idx_local) => {
                    let elem_ty = self.elem_type(cur_ty).unwrap_or(cur_ty);
                    let size    = self.type_size(elem_ty);
                    let idx     = self.local_reg(idx_local);
                    let stride  = self.alloc_reg();
                    let byte_off = self.alloc_reg();
                    let addr    = self.alloc_reg();
                    self.ops.push(LIROp::LoadImm(stride, size));
                    self.ops.push(LIROp::Mul(byte_off, idx, stride));
                    self.ops.push(LIROp::Add(addr, cur_reg, byte_off));
                    cur_reg = addr;
                    cur_ty  = elem_ty;
                    is_mem  = true;
                }
                mir::PlaceElem::ConstantIndex { offset, min_length: _, from_end: false } => {
                    let elem_ty = self.elem_type(cur_ty).unwrap_or(cur_ty);
                    let byte    = (offset as u64) * self.type_size(elem_ty);
                    if byte != 0 {
                        let off  = self.alloc_reg();
                        let addr = self.alloc_reg();
                        self.ops.push(LIROp::LoadImm(off, byte));
                        self.ops.push(LIROp::Add(addr, cur_reg, off));
                        cur_reg = addr;
                    }
                    cur_ty = elem_ty;
                    is_mem = true;
                }
                // from_end: index from the tail of the sequence.
                // For arrays the length is statically known; use min_length as the bound.
                // For slices the runtime length is in the fat pointer's metadata word (offset +8
                // from the base local), but loading it here requires back-tracking through
                // projections — use min_length as a conservative static approximation.
                mir::PlaceElem::ConstantIndex { offset, min_length, from_end: true } => {
                    let elem_ty = self.elem_type(cur_ty).unwrap_or(cur_ty);
                    let from_start = min_length.saturating_sub(offset);
                    let byte = from_start * self.type_size(elem_ty);
                    if byte != 0 {
                        let off  = self.alloc_reg();
                        let addr = self.alloc_reg();
                        self.ops.push(LIROp::LoadImm(off, byte));
                        self.ops.push(LIROp::Add(addr, cur_reg, off));
                        cur_reg = addr;
                    }
                    cur_ty = elem_ty;
                    is_mem = true;
                }
                mir::PlaceElem::Downcast(_, variant_idx) => {
                    // Type-only narrowing — no address change.
                    // Record which variant we're in so Field projections compute
                    // offsets relative to the variant layout.
                    downcast_variant = Some((cur_ty, variant_idx));
                }
                _ => { /* SubSlice, OpaqueCast — skip, best-effort */ }
            }
        }

        Ok(if is_mem { PlaceLoc::Mem(cur_reg) } else { PlaceLoc::Reg(cur_reg) })
    }

    fn lower_place_to_reg(
        &mut self,
        place: &mir::Place<'tcx>,
        body: &mir::Body<'tcx>,
    ) -> Result<Reg, String> {
        match self.lower_place(place, body)? {
            PlaceLoc::Reg(r) => Ok(r),
            PlaceLoc::Mem(ptr) => {
                let ty  = place.ty(&body.local_decls, self.tcx).ty;
                let sz  = self.type_size(ty) as u8;
                let dst = self.alloc_reg();
                if sz > 0 && sz < 8 {
                    self.ops.push(LIROp::LoadSize { dst, base: ptr, offset: 0, size: sz });
                } else {
                    self.ops.push(LIROp::Load { dst, base: ptr, offset: 0 });
                }
                Ok(dst)
            }
        }
    }

    // ── Layout helpers ─────────────────────────────────────────────────────

    fn field_byte_offset(&self, ty: ty::Ty<'tcx>, field_idx: usize) -> u64 {
        self.field_byte_offset_with_variant(ty, field_idx, None)
    }

    fn field_byte_offset_with_variant(
        &self,
        ty: ty::Ty<'tcx>,
        field_idx: usize,
        variant: Option<rustc_abi::VariantIdx>,
    ) -> u64 {
        // Fat pointer (ref/rawptr to a dyn Trait or slice): two pointer-sized fields.
        // Field 0 = data pointer (offset 0), field 1 = vtable/length pointer (offset 8).
        // layout_of fails for unsized pointees, so handle fat pointers explicitly.
        if self.is_fat_ptr_type(ty) {
            return if field_idx == 0 { 0 } else { 8 };
        }
        let env = ty::TypingEnv::fully_monomorphized();
        let layout_result = self.tcx.layout_of(env.as_query_input(ty));
        match layout_result {
            Ok(l) => {
                // For enum types accessed via a Downcast projection, use the variant
                // layout to get the correct field offset within the payload area.
                if let Some(v_idx) = variant {
                    // Try to get the variant's layout.
                    use rustc_abi::Variants;
                    match &l.variants {
                        Variants::Multiple { variants, .. } => {
                            let v_layout = &variants[v_idx];
                            // v_layout.fields gives offsets relative to the variant's
                            // own origin.  But the variant itself is positioned
                            // at the enum's "tag + padding" base, which is captured
                            // in v_layout's offset within the outer layout.
                            // The simplest correct path: ask for the field offset
                            // from the variant TyAndLayout.
                            if field_idx < v_layout.fields.count() {
                                return v_layout.fields.offset(field_idx).bytes();
                            }
                            return 0;
                        }
                        Variants::Single { .. } => {
                            // Single-variant enum or after the optimizer collapsed it.
                            // Fall through to top-level field offset.
                        }
                        _ => {}
                    }
                }
                // Top-level struct or non-variant field access.
                if field_idx < l.fields.count() {
                    l.fields.offset(field_idx).bytes()
                } else {
                    0
                }
            }
            Err(_) => 0,
        }
    }

    /// Returns true if `ty` is a fat-pointer type (&dyn Trait, *const dyn Trait,
    /// &[T], *const [T], etc.) — i.e., a pointer/ref to an unsized type.
    fn is_fat_ptr_type(&self, ty: ty::Ty<'tcx>) -> bool {
        let pointee = match ty.kind() {
            TyKind::Ref(_, inner, _)    => *inner,
            TyKind::RawPtr(inner, _)    => *inner,
            _ => return false,
        };
        // A dyn Trait or slice pointee makes the pointer fat (two words).
        matches!(pointee.kind(), TyKind::Dynamic(..) | TyKind::Slice(_) | TyKind::Str)
    }

    fn type_size(&self, ty: ty::Ty<'tcx>) -> u64 {
        let env = ty::TypingEnv::fully_monomorphized();
        self.tcx.layout_of(env.as_query_input(ty))
            .map(|l| l.size.bytes())
            .unwrap_or(8)
    }

    fn type_bits(&self, ty: ty::Ty<'tcx>) -> u64 {
        self.type_size(ty) * 8
    }

    fn type_align(&self, ty: ty::Ty<'tcx>) -> u64 {
        let env = ty::TypingEnv::fully_monomorphized();
        self.tcx.layout_of(env.as_query_input(ty))
            .map(|l| l.align.abi.bytes())
            .unwrap_or(8)
    }

    fn elem_type(&self, ty: ty::Ty<'tcx>) -> Option<ty::Ty<'tcx>> {
        match ty.kind() {
            TyKind::Array(et, _) | TyKind::Slice(et) => Some(*et),
            _ => None,
        }
    }

    /// Returns true if the type is a composite (struct/enum/tuple) that may require
    /// multiple register slots and needs to be spilled to memory for field access.
    fn is_composite_type(&self, ty: ty::Ty<'tcx>) -> bool {
        matches!(ty.kind(), TyKind::Adt(..) | TyKind::Tuple(..) | TyKind::Array(..))
    }

    // ── Rvalue lowering ────────────────────────────────────────────────────

    fn lower_rvalue_into(
        &mut self,
        dst: Reg,
        rvalue: &mir::Rvalue<'tcx>,
        body: &mir::Body<'tcx>,
    ) -> Result<(), String> {
        match rvalue {
            mir::Rvalue::Use(operand) => {
                let src = self.lower_operand(operand, body)?;
                if dst != src { self.ops.push(LIROp::Move(dst, src)); }
            }

            mir::Rvalue::BinaryOp(op, box (lhs, rhs)) => {
                let a = self.lower_operand(lhs, body)?;
                let b = self.lower_operand(rhs, body)?;
                // Use the operand type to pick signed vs unsigned variant.
                let signed = matches!(lhs.ty(&body.local_decls, self.tcx).kind(), TyKind::Int(_));
                let insn = match op {
                    mir::BinOp::Add | mir::BinOp::AddUnchecked => LIROp::Add(dst, a, b),
                    mir::BinOp::Sub | mir::BinOp::SubUnchecked => LIROp::Sub(dst, a, b),
                    mir::BinOp::Mul | mir::BinOp::MulUnchecked => LIROp::Mul(dst, a, b),
                    mir::BinOp::Div => if signed { LIROp::SDiv(dst,a,b) } else { LIROp::Div(dst,a,b) },
                    mir::BinOp::Rem => LIROp::Rem(dst, a, b),
                    mir::BinOp::BitAnd => LIROp::And(dst, a, b),
                    mir::BinOp::BitOr  => LIROp::Or(dst, a, b),
                    mir::BinOp::BitXor => LIROp::Xor(dst, a, b),
                    mir::BinOp::Shl | mir::BinOp::ShlUnchecked => LIROp::Shl(dst, a, b),
                    mir::BinOp::Shr | mir::BinOp::ShrUnchecked =>
                        if signed { LIROp::Sar(dst,a,b) } else { LIROp::Shr(dst,a,b) },
                    mir::BinOp::Eq  => LIROp::Eq(dst, a, b),
                    mir::BinOp::Ne  => LIROp::Ne(dst, a, b),
                    mir::BinOp::Lt  => if signed { LIROp::SLt(dst,a,b) } else { LIROp::Lt(dst,a,b) },
                    mir::BinOp::Le  => if signed { LIROp::SLe(dst,a,b) } else { LIROp::Le(dst,a,b) },
                    mir::BinOp::Gt  => if signed { LIROp::SGt(dst,a,b) } else { LIROp::Gt(dst,a,b) },
                    mir::BinOp::Ge  => if signed { LIROp::SGe(dst,a,b) } else { LIROp::Ge(dst,a,b) },
                    mir::BinOp::Offset => { LIROp::Add(dst, a, b) } // ptr + isize
                    // WithOverflow variants: returns (T, bool). With panic=abort,
                    // pack the result in low bits; overflow flag (high bits) = 0.
                    mir::BinOp::AddWithOverflow => LIROp::Add(dst, a, b),
                    mir::BinOp::SubWithOverflow => LIROp::Sub(dst, a, b),
                    mir::BinOp::MulWithOverflow => LIROp::Mul(dst, a, b),
                    // Cmp: signed three-way comparison → Ordering (-1/0/1 as i8)
                    mir::BinOp::Cmp => {
                        // Emit: lt = (a < b); gt = (a > b); result = gt - lt
                        let lt = self.alloc_reg();
                        let gt = self.alloc_reg();
                        self.ops.push(LIROp::SLt(lt, a, b));
                        self.ops.push(LIROp::SGt(gt, a, b));
                        self.ops.push(LIROp::Sub(dst, gt, lt));
                        return Ok(());
                    }
                    _ => {
                        self.ops.push(LIROp::Comment(format!("unhandled binop {:?}", op)));
                        return Ok(());
                    }
                };
                self.ops.push(insn);
            }

            mir::Rvalue::UnaryOp(op, operand) => {
                match op {
                    mir::UnOp::Not => {
                        let s = self.lower_operand(operand, body)?;
                        self.ops.push(LIROp::Not(dst, s));
                    }
                    mir::UnOp::Neg => {
                        let s = self.lower_operand(operand, body)?;
                        self.ops.push(LIROp::Neg(dst, s));
                    }
                    mir::UnOp::PtrMetadata => {
                        // `PtrMetadata(*fat_ptr)` extracts the metadata word (vtable ptr or length).
                        // The operand is a fat pointer stored in memory (a reg holding its address).
                        // Fat pointers are two consecutive 8-byte words: [data_ptr, metadata].
                        // Lower the operand's place to a memory address, then load at offset +8.
                        if let mir::Operand::Copy(place) | mir::Operand::Move(place) = operand {
                            match self.lower_place(place, body)? {
                                PlaceLoc::Mem(ptr) => {
                                    self.ops.push(LIROp::Load { dst, base: ptr, offset: 8 });
                                }
                                PlaceLoc::Reg(r) => {
                                    // Fat ptr is in a register: metadata is in the high word.
                                    // Emit a shift to extract upper 64 bits — but fat ptrs are
                                    // typically in memory. Fall back: treat reg as address and load.
                                    self.ops.push(LIROp::Load { dst, base: r, offset: 8 });
                                }
                            }
                        } else {
                            self.ops.push(LIROp::LoadImm(dst, 0));
                        }
                    }
                }
            }

            mir::Rvalue::Cast(kind, operand, to_ty) => {
                let src = self.lower_operand(operand, body)?;
                self.lower_cast(dst, src, *kind, operand.ty(&body.local_decls, self.tcx), *to_ty);
            }

            // &(*ptr) or &raw const (*ptr) — the address is the pointer value itself.
            mir::Rvalue::Ref(_, _, place) | mir::Rvalue::RawPtr(_, place) => {
                if place.projection.len() == 1
                    && matches!(place.projection[0], mir::PlaceElem::Deref)
                {
                    let base = self.local_reg(place.local);
                    if dst != base { self.ops.push(LIROp::Move(dst, base)); }
                } else if place.projection.is_empty() {
                    // Taking address of a plain local. Allocate a writable BSS slot,
                    // store the local's current value there, return its address.
                    // This is correct for read-only references and short-lived borrows.
                    let val = self.local_reg(place.local);
                    let ty  = body.local_decls[place.local].ty;
                    let sz  = self.type_size(ty).max(1) as usize;
                    let sym = format!("__local_ref_{}", self.agg_counter);
                    self.agg_counter += 1;
                    self.statics.push(StaticData { name: sym.clone(), bytes: vec![0u8; sz], writable: true });
                    self.ops.push(LIROp::LoadAddr { dst, symbol: sym.clone() });
                    let size_u8 = sz as u8;
                    if sz > 0 && sz < 8 {
                        self.ops.push(LIROp::StoreSize { src: val, base: dst, offset: 0, size: size_u8 });
                    } else {
                        self.ops.push(LIROp::Store { src: val, base: dst, offset: 0 });
                    }
                } else {
                    // General case: compute the address of the place.
                    match self.lower_place(place, body)? {
                        PlaceLoc::Reg(r) => { if dst != r { self.ops.push(LIROp::Move(dst, r)); } }
                        PlaceLoc::Mem(ptr) => { if dst != ptr { self.ops.push(LIROp::Move(dst, ptr)); } }
                    }
                }
            }

            mir::Rvalue::Aggregate(kind, fields) => {
                if fields.len() == 1 {
                    let src = self.lower_operand(&fields[rustc_abi::FieldIdx::from_usize(0)], body)?;
                    if dst != src { self.ops.push(LIROp::Move(dst, src)); }
                } else if fields.is_empty() {
                    self.ops.push(LIROp::LoadImm(dst, 0));
                } else {
                    // Multi-field aggregate: store each field into a named static block,
                    // then dst holds the pointer to it. (Phase 3 best-effort: works for
                    // small structs that are only accessed via field projections on the ptr.)
                    self.ops.push(LIROp::Comment(format!("aggregate {:?} {} fields", kind, fields.len())));
                    // Compute each field and store at consecutive offsets in a fresh stack slot.
                    // We use an alloc_reg to hold the base pointer, and Store ops for each field.
                    // The caller (lower_statement) will then access fields via Load from dst.
                    // For now, store each field offset into a fake "inline struct" area by
                    // using StoreSize ops relative to dst — which must point to writable memory.
                    // Since we can't allocate real stack here (no LIR for that), use a simple
                    // packing strategy: lower each field, pack into a single 64-bit reg if small.
                    let mut packed: u64 = 0;
                    let mut all_imm = true;
                    for (idx, _) in fields.iter_enumerated() {
                        let fld = &fields[idx];
                        if let mir::Operand::Constant(c) = fld {
                            let typing_env = ty::TypingEnv::fully_monomorphized();
                            if let Some(bits) = c.const_.try_eval_bits(self.tcx, typing_env) {
                                let shift = idx.as_usize() * 32;
                                if shift < 64 {
                                    packed |= (bits as u64 & 0xFFFF_FFFF) << shift;
                                }
                                continue;
                            }
                        }
                        all_imm = false;
                        break;
                    }
                    if all_imm && fields.len() <= 2 {
                        // Small struct with constant fields: pack into one register.
                        self.ops.push(LIROp::LoadImm(dst, packed));
                    } else {
                        // General case: allocate writable BSS static for the aggregate.
                        let agg_id = self.agg_counter;
                        self.agg_counter += 1;
                        let field_tys: Vec<ty::Ty<'tcx>> = fields.iter_enumerated()
                            .map(|(_, op)| op.ty(&body.local_decls, self.tcx))
                            .collect();
                        let (offsets, total) = self.sequential_offsets(&field_tys);
                        let sym = format!("__agg_{agg_id}");
                        self.statics.push(StaticData {
                            name: sym.clone(),
                            bytes: vec![0u8; total as usize],
                            writable: true,
                        });
                        self.ops.push(LIROp::LoadAddr { dst, symbol: sym });
                        for (idx, _) in fields.iter_enumerated() {
                            let src = self.lower_operand(&fields[idx], body)?;
                            let off = offsets[idx.as_usize()] as i32;
                            let sz  = self.type_size(field_tys[idx.as_usize()]) as u8;
                            if sz > 0 && sz < 8 {
                                self.ops.push(LIROp::StoreSize { src, base: dst, offset: off, size: sz });
                            } else {
                                self.ops.push(LIROp::Store { src, base: dst, offset: off });
                            }
                        }
                    }
                }
            }

            mir::Rvalue::Discriminant(place) => {
                // Read the discriminant of an enum. The discriminant is at byte offset 0
                // of the enum's memory layout. We need the address of the enum, not its
                // value — use lower_place to get the PlaceLoc and handle both cases.
                match self.lower_place(place, body)? {
                    PlaceLoc::Mem(ptr) => {
                        // Load discriminant (4-byte i32, zero-extended to 64 bits).
                        self.ops.push(LIROp::LoadSize { dst, base: ptr, offset: 0, size: 4 });
                    }
                    PlaceLoc::Reg(r) => {
                        // Enum is in a register (unlikely for composite, but handle it).
                        // Discriminant is in the low 32 bits.
                        self.ops.push(LIROp::ZeroExt { dst, src: r, from_bits: 32 });
                    }
                }
            }

            // NullaryOp in this rustc version only carries RuntimeChecks (ub_checks etc.).
            // Return 0 to disable all runtime checks in our backend.
            mir::Rvalue::NullaryOp(_) => {
                self.ops.push(LIROp::LoadImm(dst, 0));
            }

            // ShallowInitBox: the operand is the raw pointer for a box allocation.
            mir::Rvalue::ShallowInitBox(operand, _ty) => {
                let src = self.lower_operand(operand, body)?;
                if dst != src { self.ops.push(LIROp::Move(dst, src)); }
            }

            // CopyForDeref: semantically identical to Use(Copy(place)).
            mir::Rvalue::CopyForDeref(place) => {
                let src = self.lower_place_to_reg(place, body)?;
                if dst != src { self.ops.push(LIROp::Move(dst, src)); }
            }

            // ThreadLocalRef: generate a pthread-key-based TLS access.
            // __tls_key_<sym> is an 8-byte BSS slot holding the pthread_key_t (0 = not yet
            // created). __trident_tls_get(key_ptr, size) lazily creates the key and returns
            // the per-thread data pointer. The macho-linker path resolves _pthread_* via
            // libSystem stubs; the monolithic path leaves those calls unresolved (TLS only
            // works correctly through Phase 2+).
            mir::Rvalue::ThreadLocalRef(def_id) => {
                let inst     = ty::Instance::mono(self.tcx, *def_id);
                let sym      = self.tcx.symbol_name(inst).name.to_string();
                let ty       = self.tcx.type_of(*def_id).instantiate_identity();
                let size     = self.type_size(ty).max(1);
                let key_slot = format!("__tls_key_{sym}");
                self.tls_vars.insert(key_slot.clone(), size);
                let key_ptr = self.alloc_reg();
                let sz_reg  = self.alloc_reg();
                self.ops.push(LIROp::LoadAddr { dst: key_ptr, symbol: key_slot });
                self.ops.push(LIROp::LoadImm(sz_reg, size));
                self.ops.push(LIROp::Move(Reg(0), key_ptr));
                self.ops.push(LIROp::Move(Reg(1), sz_reg));
                self.ops.push(LIROp::Call("__trident_tls_get".to_string()));
                self.ops.push(LIROp::Move(dst, Reg(0)));
            }

            // Repeat(operand, count): allocate a writable BSS block of count×elem_size bytes,
            // fill it with copies of operand, return its address.
            mir::Rvalue::Repeat(operand, count) => {
                let n = count.try_to_target_usize(self.tcx).unwrap_or(0);
                let elem_ty  = operand.ty(&body.local_decls, self.tcx);
                let elem_sz  = self.type_size(elem_ty).max(1);
                let total    = n * elem_sz;
                let agg_id   = self.agg_counter;
                self.agg_counter += 1;
                let sym = format!("__repeat_{agg_id}");
                self.statics.push(StaticData {
                    name: sym.clone(),
                    bytes: vec![0u8; total as usize],
                    writable: true,
                });
                self.ops.push(LIROp::LoadAddr { dst, symbol: sym.clone() });
                if n > 0 {
                    let src = self.lower_operand(operand, body)?;
                    if n <= 8 {
                        for i in 0..n {
                            let off = (i * elem_sz) as i32;
                            let sz  = elem_sz as u8;
                            if sz > 0 && sz < 8 {
                                self.ops.push(LIROp::StoreSize { src, base: dst, offset: off, size: sz });
                            } else {
                                self.ops.push(LIROp::Store { src, base: dst, offset: off });
                            }
                        }
                    } else {
                        // Fill via __trident_memset(dst, val, count*elem_sz).
                        // Note: correct only when elem_sz == 1 (byte fill). For multi-byte
                        // elements with non-zero values this is approximate; zero-init patterns
                        // (the common [0u8; N]) are correct since BSS is already zero.
                        let n_reg  = self.alloc_reg();
                        let sz_reg = self.alloc_reg();
                        let bc     = self.alloc_reg();
                        self.ops.push(LIROp::LoadImm(n_reg, n));
                        self.ops.push(LIROp::LoadImm(sz_reg, elem_sz));
                        self.ops.push(LIROp::Mul(bc, n_reg, sz_reg));
                        self.ops.push(LIROp::Move(Reg(0), dst));
                        self.ops.push(LIROp::Move(Reg(1), src));
                        self.ops.push(LIROp::Move(Reg(2), bc));
                        self.ops.push(LIROp::Call("__trident_memset".to_string()));
                        // Reload dst: __trident_memset post-increments x0, so base is gone.
                        self.ops.push(LIROp::LoadAddr { dst, symbol: sym });
                    }
                }
            }

            _ => {
                self.ops.push(LIROp::Comment(format!("rvalue: {:?}", rvalue)));
                self.ops.push(LIROp::LoadImm(dst, 0));
            }
        }
        Ok(())
    }

    // ── Cast lowering ──────────────────────────────────────────────────────

    fn lower_cast(
        &mut self,
        dst: Reg,
        src: Reg,
        kind: mir::CastKind,
        from_ty: ty::Ty<'tcx>,
        _to_ty: ty::Ty<'tcx>,
    ) {
        match kind {
            mir::CastKind::IntToInt => {
                let from_bits = int_bits(from_ty);
                let to_bits   = int_bits(_to_ty);
                let signed    = matches!(from_ty.kind(), TyKind::Int(_));
                if to_bits < from_bits {
                    // Truncation: mask out upper bits so they don't leak.
                    if to_bits <= 32 {
                        self.ops.push(LIROp::ZeroExt { dst, src, from_bits: to_bits as u8 });
                    } else {
                        if dst != src { self.ops.push(LIROp::Move(dst, src)); }
                    }
                } else if to_bits > from_bits {
                    if signed {
                        self.ops.push(LIROp::SignExt { dst, src, from_bits: from_bits as u8 });
                    } else {
                        self.ops.push(LIROp::ZeroExt { dst, src, from_bits: from_bits as u8 });
                    }
                } else {
                    if dst != src { self.ops.push(LIROp::Move(dst, src)); }
                }
            }
            // &ConcreteType → &dyn Trait: build a fat pointer {data_ptr, vtable_ptr} in BSS.
            mir::CastKind::PointerCoercion(PointerCoercion::Unsize, _) => {
                if let Some(vtable_sym) = self.make_vtable_for_unsize(from_ty, _to_ty) {
                    // If the vtable is in BSS (writable), initialize it now with ADRP+ADD
                    // addresses. This is position-independent and works correctly with ASLR.
                    let vtable_is_bss = self.statics.iter()
                        .find(|s| s.name == vtable_sym)
                        .map(|s| s.writable)
                        .unwrap_or(false);
                    if vtable_is_bss {
                        // Collect init data before borrowing self mutably.
                        let vtable_bytes: Vec<u8> = self.statics.iter()
                            .find(|s| s.name == vtable_sym)
                            .map(|s| s.bytes.clone())
                            .unwrap_or_default();
                        let fn_relocs: Vec<(usize, String)> = self.static_relocs.iter()
                            .filter(|r| r.static_name == vtable_sym)
                            .map(|r| (r.byte_offset, r.fn_symbol.clone()))
                            .collect();
                        let fn_offsets: std::collections::HashSet<usize> =
                            fn_relocs.iter().map(|(o, _)| *o).collect();

                        let r_vt = self.alloc_reg();
                        self.ops.push(LIROp::LoadAddr { dst: r_vt, symbol: vtable_sym.clone() });

                        // Write constant fields (size, align, etc.) that are non-zero.
                        let mut off = 0usize;
                        while off + 8 <= vtable_bytes.len() {
                            if !fn_offsets.contains(&off) {
                                let val = u64::from_le_bytes(
                                    vtable_bytes[off..off + 8].try_into().unwrap()
                                );
                                if val != 0 {
                                    let r_v = self.alloc_reg();
                                    self.ops.push(LIROp::LoadImm(r_v, val));
                                    self.ops.push(LIROp::Store {
                                        src: r_v, base: r_vt, offset: off as i32,
                                    });
                                }
                            }
                            off += 8;
                        }
                        // Write function pointer slots via ADRP+ADD (position-independent).
                        for (byte_off, fn_sym) in fn_relocs {
                            let r_fn = self.alloc_reg();
                            self.ops.push(LIROp::LoadAddr { dst: r_fn, symbol: fn_sym });
                            self.ops.push(LIROp::Store {
                                src: r_fn, base: r_vt, offset: byte_off as i32,
                            });
                        }
                    }

                    let fp_sym = format!("__fat_ptr_{}", self.agg_counter);
                    self.agg_counter += 1;
                    self.statics.push(StaticData {
                        name: fp_sym.clone(), bytes: vec![0u8; 16], writable: true,
                    });
                    let fp_addr = self.alloc_reg();
                    self.ops.push(LIROp::LoadAddr { dst: fp_addr, symbol: fp_sym });
                    // Store the concrete data pointer at offset 0.
                    self.ops.push(LIROp::Store { src, base: fp_addr, offset: 0 });
                    // Store the vtable pointer at offset 8.
                    let vt = self.alloc_reg();
                    self.ops.push(LIROp::LoadAddr { dst: vt, symbol: vtable_sym });
                    self.ops.push(LIROp::Store { src: vt, base: fp_addr, offset: 8 });
                    if dst != fp_addr { self.ops.push(LIROp::Move(dst, fp_addr)); }
                } else {
                    if dst != src { self.ops.push(LIROp::Move(dst, src)); }
                }
            }
            // All other pointer/transmute/fn-ptr casts: same bit pattern, just move.
            _ => {
                if dst != src { self.ops.push(LIROp::Move(dst, src)); }
            }
        }
    }

    /// Get the raw memory pointer for a composite operand (mem-backed place or constant).
    /// For mem_locals, returns the pointer register directly without loading.
    /// For non-mem-backed places of composite type, spills to a new BSS static first.
    fn lower_composite_arg_ptr(
        &mut self,
        operand: &mir::Operand<'tcx>,
        body: &mir::Body<'tcx>,
    ) -> Result<Reg, String> {
        match operand {
            mir::Operand::Move(place) | mir::Operand::Copy(place) => {
                if place.projection.is_empty() && self.mem_locals.contains(&place.local) {
                    // Already memory-backed; local_reg holds the pointer.
                    return Ok(self.local_reg(place.local));
                }
                // Not memory-backed yet — spill to BSS.
                let ty = place.ty(&body.local_decls, self.tcx).ty;
                let size = self.type_size(ty) as usize;
                let sym = format!("__carg_{}", self.agg_counter);
                self.agg_counter += 1;
                self.statics.push(StaticData {
                    name: sym.clone(),
                    bytes: vec![0u8; size],
                    writable: true,
                });
                let ptr = self.alloc_reg();
                self.ops.push(LIROp::LoadAddr { dst: ptr, symbol: sym });
                // Copy slots from the operand's register(s) into BSS.
                // Since it's not mem-backed, it may just be a single register value.
                let val = self.lower_place_to_reg(place, body)?;
                self.ops.push(LIROp::Store { src: val, base: ptr, offset: 0 });
                Ok(ptr)
            }
            mir::Operand::Constant(_) => {
                // Constant composite — lower via standard path, then if the result
                // is a pointer (BSS from intern_alloc), return it directly.
                let val = self.lower_operand(operand, body)?;
                Ok(val)
            }
        }
    }

    // ── Operand lowering ───────────────────────────────────────────────────

    fn lower_operand(
        &mut self,
        operand: &mir::Operand<'tcx>,
        body: &mir::Body<'tcx>,
    ) -> Result<Reg, String> {
        match operand {
            mir::Operand::Move(place) | mir::Operand::Copy(place) => {
                self.lower_place_to_reg(place, body)
            }
            mir::Operand::Constant(c) => {
                let dst = self.alloc_reg();
                let typing_env = ty::TypingEnv::fully_monomorphized();
                if let Some(bits) = c.const_.try_eval_bits(self.tcx, typing_env) {
                    self.ops.push(LIROp::LoadImm(dst, bits as u64));
                    return Ok(dst);
                }
                if let mir::Const::Val(ConstValue::Scalar(Scalar::Ptr(ptr, _)), _) = c.const_ {
                    if let GlobalAlloc::Static(def_id) = self.tcx.global_alloc(ptr.provenance.alloc_id()) {
                        let sym = self.tcx.symbol_name(ty::Instance::mono(self.tcx, def_id))
                            .name.to_string();
                        self.ops.push(LIROp::LoadAddr { dst, symbol: sym });
                        return Ok(dst);
                    }
                }
                // Byte string or slice constant
                if let mir::Const::Val(ConstValue::Indirect { alloc_id, offset }, ty) = c.const_ {
                    if let Some(sym) = self.intern_alloc(alloc_id, ty) {
                        self.ops.push(LIROp::LoadAddr { dst, symbol: sym });
                        if offset.bytes() != 0 {
                            let off = self.alloc_reg();
                            self.ops.push(LIROp::LoadImm(off, offset.bytes()));
                            self.ops.push(LIROp::Add(dst, dst, off));
                        }
                        return Ok(dst);
                    }
                }
                // FnDef constant → load the function's address.
                if let TyKind::FnDef(def_id, substs) = c.const_.ty().kind() {
                    let inst = Instance::try_resolve(self.tcx, ty::TypingEnv::fully_monomorphized(), *def_id, substs)
                        .ok().flatten()
                        .unwrap_or_else(|| Instance::mono(self.tcx, *def_id));
                    let sym = self.tcx.symbol_name(inst).name.to_string();
                    self.ops.push(LIROp::LoadAddr { dst, symbol: sym });
                    return Ok(dst);
                }
                self.ops.push(LIROp::Comment(format!("const: {:?}", c)));
                self.ops.push(LIROp::LoadImm(dst, 0));
                Ok(dst)
            }
        }
    }

    fn intern_alloc(&mut self, alloc_id: rustc_middle::mir::interpret::AllocId, _ty: ty::Ty<'tcx>) -> Option<String> {
        if let GlobalAlloc::Memory(alloc) = self.tcx.global_alloc(alloc_id) {
            let name = format!("__anon_const_{:?}", alloc_id);
            // Deduplicate: vtable allocs shared across coercions must only emit once.
            if self.statics.iter().any(|s| s.name == name) {
                return Some(name);
            }
            let inner = alloc.inner();
            let len = inner.len();
            let mut bytes = inner.inspect_with_uninit_and_ptr_outside_interpreter(0..len).to_vec();
            // Collect pointer-sized relocations (function pointers in vtables, etc.).
            // Each provenance entry maps a byte offset to an alloc that may be a function.
            let relocs_before = self.static_relocs.len();
            for (offset, prov) in inner.provenance().ptrs().iter() {
                let byte_off = offset.bytes() as usize;
                if byte_off + 8 > bytes.len() { continue; }
                let prov_alloc_id = prov.alloc_id();
                match self.tcx.global_alloc(prov_alloc_id) {
                    GlobalAlloc::Function { instance } => {
                        let sym = self.tcx.symbol_name(instance).name.to_string();
                        // Zero out the pointer slot; the linker/emitter will patch it.
                        bytes[byte_off..byte_off + 8].fill(0);
                        self.static_relocs.push(StaticReloc {
                            static_name: name.clone(),
                            byte_offset: byte_off,
                            fn_symbol:   sym,
                        });
                    }
                    GlobalAlloc::Static(def_id) => {
                        // Pointer to another static — record as data-to-data; zero the slot.
                        let inst = ty::Instance::mono(self.tcx, def_id);
                        let sym = self.tcx.symbol_name(inst).name.to_string();
                        bytes[byte_off..byte_off + 8].fill(0);
                        self.static_relocs.push(StaticReloc {
                            static_name: name.clone(),
                            byte_offset: byte_off,
                            fn_symbol:   sym,
                        });
                    }
                    _ => {}
                }
            }
            // Vtables with fn-ptr slots need runtime initialization (LoadAddr = ADRP+ADD,
            // position-independent). Mark them writable (BSS) so they can be written at runtime.
            let has_fn_ptrs = self.static_relocs.len() > relocs_before;
            self.statics.push(StaticData { name: name.clone(), bytes, writable: has_fn_ptrs });
            return Some(name);
        }
        None
    }

    /// Build a vtable for a `&ConcreteType → &dyn Trait` coercion and intern it as a static.
    /// Returns the symbol name of the vtable, or None if the types don't match the expected shape.
    fn make_vtable_for_unsize(&mut self, from_ty: ty::Ty<'tcx>, to_ty: ty::Ty<'tcx>) -> Option<String> {
        let concrete_ty = match from_ty.kind() {
            TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => *inner,
            _ => return None,
        };
        let to_pointee = match to_ty.kind() {
            TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => *inner,
            _ => return None,
        };
        let preds = match to_pointee.kind() {
            TyKind::Dynamic(preds, ..) => preds,
            _ => return None,
        };
        // skip_binder is correct at full monomorphization: no unresolved bound regions remain.
        let existential_trait_ref = preds.principal().map(|p| p.skip_binder());
        let vtable_alloc_id = self.tcx.vtable_allocation((concrete_ty, existential_trait_ref));
        self.intern_alloc(vtable_alloc_id, concrete_ty)
    }

    /// If `func` resolves to a virtual dispatch (`InstanceKind::Virtual`), emit LIROps that load
    /// the concrete function pointer from the vtable BEFORE args are moved into x0..xN.
    /// Returns the register holding the function pointer, or None for non-virtual calls.
    fn resolve_virtual_fn_ptr(
        &mut self,
        func: &mir::Operand<'tcx>,
        args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
        body: &mir::Body<'tcx>,
    ) -> Result<Option<Reg>, String> {
        let mir::Operand::Constant(c) = func else { return Ok(None); };
        let TyKind::FnDef(def_id, substs) = c.const_.ty().kind() else { return Ok(None); };
        let mono_substs = self.tcx.instantiate_and_normalize_erasing_regions(
            self.instance.args,
            ty::TypingEnv::fully_monomorphized(),
            ty::EarlyBinder::bind(*substs),
        );
        let inst_opt = Instance::try_resolve(
            self.tcx, ty::TypingEnv::fully_monomorphized(), *def_id, mono_substs,
        ).ok().flatten().or_else(|| {
            if self.tcx.generics_of(*def_id).count() == 0 {
                Some(Instance::mono(self.tcx, *def_id))
            } else { None }
        });
        let Some(inst) = inst_opt else { return Ok(None); };
        let ty::InstanceKind::Virtual(_, vtable_idx) = inst.def else { return Ok(None); };

        // arg[0] is the fat pointer; its register holds the BSS address of {data_ptr, vtable_ptr}.
        let arg0 = args.first().ok_or_else(|| "virtual call with no self argument".to_string())?;
        let fat_ptr_addr = self.lower_operand(&arg0.node, body)?;

        // Load vtable_ptr (offset 8 in the fat pointer).
        let vtable_ptr = self.alloc_reg();
        self.ops.push(LIROp::Load { dst: vtable_ptr, base: fat_ptr_addr, offset: 8 });

        // Load fn ptr from vtable. vtable_idx already includes header (0=drop,1=size,2=align).
        let fn_ptr = self.alloc_reg();
        let fn_off = (vtable_idx * 8) as i32;
        self.ops.push(LIROp::Load { dst: fn_ptr, base: vtable_ptr, offset: fn_off });

        // Ensure the fn ptr isn't in an arg register where arg setup would clobber it.
        let safe = if fn_ptr.0 < 8 {
            let s = self.alloc_reg();
            self.ops.push(LIROp::Move(s, fn_ptr));
            s
        } else {
            fn_ptr
        };
        Ok(Some(safe))
    }

    // Sequential C-style field layout using individual field types.
    fn sequential_offsets(&self, field_tys: &[ty::Ty<'tcx>]) -> (Vec<u64>, u64) {
        let mut offsets = Vec::with_capacity(field_tys.len());
        let mut cur = 0u64;
        let mut max_align = 1u64;
        for &ty in field_tys {
            let align = self.type_align(ty);
            max_align = max_align.max(align);
            cur = (cur + align - 1) & !(align - 1);
            offsets.push(cur);
            cur += self.type_size(ty);
        }
        let total = if field_tys.is_empty() { 0 } else { (cur + max_align - 1) & !(max_align - 1) };
        (offsets, total.max(1))
    }

    // ── Terminator lowering ────────────────────────────────────────────────

    fn lower_terminator(
        &mut self,
        term: &mir::Terminator<'tcx>,
        body: &mir::Body<'tcx>,
    ) -> Result<(), String> {
        match &term.kind {
            mir::TerminatorKind::Return => {
                self.ops.push(LIROp::Return);
            }

            mir::TerminatorKind::Unreachable => {
                self.ops.push(LIROp::Halt);
            }

            mir::TerminatorKind::Goto { target } => {
                self.ops.push(LIROp::Jump(bb_label(*target)));
            }

            // Drop: call drop_in_place::<T> when the type has a non-trivial destructor.
            mir::TerminatorKind::Drop { place, target, .. } => {
                let ty = place.ty(&body.local_decls, self.tcx).ty;
                let typing_env = ty::TypingEnv::fully_monomorphized();
                if ty.needs_drop(self.tcx, typing_env) {
                    let def_id = self.tcx.require_lang_item(rustc_hir::LangItem::DropInPlace, rustc_span::DUMMY_SP);
                    let args = self.tcx.mk_args(&[ty.into()]);
                    let drop_inst = Instance::try_resolve(self.tcx, typing_env, def_id, args)
                        .ok().flatten();
                    if let Some(inst) = drop_inst {
                        let ptr = match self.lower_place(place, body)? {
                            PlaceLoc::Mem(ptr) => ptr,
                            PlaceLoc::Reg(r)   => r,
                        };
                        self.ops.push(LIROp::Move(Reg(0), ptr));
                        let sym = self.tcx.symbol_name(inst).name.to_string();
                        self.ops.push(LIROp::Call(sym));
                    }
                }
                self.ops.push(LIROp::Jump(bb_label(*target)));
            }

            // Unwind paths in panic=abort programs terminate execution.
            mir::TerminatorKind::UnwindResume | mir::TerminatorKind::UnwindTerminate(_) => {
                self.ops.push(LIROp::Halt);
            }

            mir::TerminatorKind::Assert { cond, expected, target, unwind: _, msg: _ } => {
                // With panic=abort: if assertion fails, call exit(101).
                let c = self.lower_operand(cond, body)?;
                let pass = self.alloc_reg();
                let fail_label = Label::new(format!("__assert_fail_{}", self.next_vreg));
                if *expected {
                    // cond must be true → branch to fail if cond==0
                    self.ops.push(LIROp::LoadImm(pass, 1));
                    self.ops.push(LIROp::Eq(pass, c, pass));
                    self.ops.push(LIROp::Branch {
                        cond: c,
                        if_true: bb_label(*target),
                        if_false: fail_label.clone(),
                    });
                } else {
                    // cond must be false → branch to fail if cond!=0
                    self.ops.push(LIROp::Branch {
                        cond: c,
                        if_true: fail_label.clone(),
                        if_false: bb_label(*target),
                    });
                }
                self.ops.push(LIROp::LabelDef(fail_label));
                self.ops.push(LIROp::Halt); // abort
            }

            // FalseEdge and FalseUnwind exist only for borrow-checker; real target is the first.
            mir::TerminatorKind::FalseEdge { real_target, .. }
            | mir::TerminatorKind::FalseUnwind { real_target, .. } => {
                self.ops.push(LIROp::Jump(bb_label(*real_target)));
            }

            mir::TerminatorKind::SwitchInt { discr, targets } => {
                let cond = self.lower_operand(discr, body)?;
                let otherwise = targets.otherwise();
                let cases: Vec<_> = targets.iter().collect();
                if cases.len() == 1 {
                    let (val, then_bb) = cases[0];
                    let tmp = self.alloc_reg();
                    let eq  = self.alloc_reg();
                    self.ops.push(LIROp::LoadImm(tmp, val as u64));
                    self.ops.push(LIROp::Eq(eq, cond, tmp));
                    self.ops.push(LIROp::Branch {
                        cond: eq,
                        if_true:  bb_label(then_bb),
                        if_false: bb_label(otherwise),
                    });
                } else {
                    for (val, bb) in &cases {
                        let tmp  = self.alloc_reg();
                        let eq   = self.alloc_reg();
                        let next = Label::new(format!("__sw_{}", self.next_vreg));
                        self.ops.push(LIROp::LoadImm(tmp, *val as u64));
                        self.ops.push(LIROp::Eq(eq, cond, tmp));
                        self.ops.push(LIROp::Branch {
                            cond: eq,
                            if_true:  bb_label(*bb),
                            if_false: next.clone(),
                        });
                        self.ops.push(LIROp::LabelDef(next));
                    }
                    self.ops.push(LIROp::Jump(bb_label(otherwise)));
                }
            }

            mir::TerminatorKind::Call { func, args, destination, target, .. } => {
                // Determine callee first so we can handle atomics before arg setup.
                let is_direct_fn_def = matches!(func,
                    mir::Operand::Constant(c)
                    if matches!(c.const_.ty().kind(), TyKind::FnDef(..))
                );

                if is_direct_fn_def {
                    if let mir::Operand::Constant(c) = func {
                        if let TyKind::FnDef(def_id, substs) = c.const_.ty().kind() {
                            if let Some(intr) = self.tcx.intrinsic(*def_id) {
                                // Const intrinsics (size_of, align_of, ...) before arg setup.
                                if self.lower_const_intrinsic(intr.name.as_str(), substs, destination, target, body)? {
                                    return Ok(());
                                }
                                // Atomic intrinsics before arg setup.
                                if self.lower_atomic(intr.name.as_str(), args, destination, target, body)? {
                                    return Ok(());
                                }
                                if self.lower_misc_intrinsic(intr.name.as_str(), substs, args, destination, target, body)? {
                                    return Ok(());
                                }
                            }
                        }
                    }
                }

                // For virtual dispatch: resolve the vtable fn ptr BEFORE arg setup so arg
                // moves can't clobber the intermediate registers.
                let virtual_fn_ptr = if is_direct_fn_def {
                    self.resolve_virtual_fn_ptr(func, args, body)?
                } else {
                    None
                };

                // For indirect calls, evaluate the function pointer into a fresh scratch
                // register BEFORE setting up arguments, so arg moves don't clobber it.
                let indirect_fn_ptr = if !is_direct_fn_def {
                    let fp = self.lower_operand(func, body)?;
                    // If fp is in an argument register (Reg(0..7)), move it to safety.
                    if fp.0 < 8 {
                        let scratch = self.alloc_reg();
                        self.ops.push(LIROp::Move(scratch, fp));
                        Some(scratch)
                    } else {
                        Some(fp)
                    }
                } else {
                    None
                };

                // Move arguments into x0..xN.
                // Virtual calls: arg[0] is the fat pointer — load only data_ptr (offset 0) into x0.
                // For composite types (size > 8 bytes), the arg is memory-backed:
                // expand it into consecutive register slots by loading 8-byte chunks.
                if virtual_fn_ptr.is_some() {
                    // x0 = fat_ptr.data_ptr; x1..xN = remaining args.
                    if let Some(arg0) = args.first() {
                        let fat_ptr_addr = self.lower_operand(&arg0.node, body)?;
                        self.ops.push(LIROp::Load { dst: Reg(0), base: fat_ptr_addr, offset: 0 });
                    }
                    for (i, arg) in args.iter().skip(1).enumerate() {
                        let src = self.lower_operand(&arg.node, body)?;
                        let reg_idx = (i + 1) as u32;
                        if src.0 != reg_idx { self.ops.push(LIROp::Move(Reg(reg_idx), src)); }
                    }
                } else {
                    let mut reg_slot = 0u32;
                    for arg in args.iter() {
                        let arg_ty = arg.node.ty(&body.local_decls, self.tcx);
                        let arg_size = self.type_size(arg_ty);
                        let slots = if arg_size <= 8 { 1u32 } else { ((arg_size + 7) / 8) as u32 };

                        if slots > 1 && self.is_composite_type(arg_ty) {
                            // Arg lives in memory; get the backing pointer without loading.
                            let ptr = self.lower_composite_arg_ptr(&arg.node, body)?;
                            for slot in 0..slots {
                                let off = (slot * 8) as i32;
                                let dst_reg = Reg(reg_slot + slot);
                                self.ops.push(LIROp::Load { dst: dst_reg, base: ptr, offset: off });
                            }
                        } else {
                            let src = self.lower_operand(&arg.node, body)?;
                            if src.0 != reg_slot {
                                self.ops.push(LIROp::Move(Reg(reg_slot), src));
                            }
                        }
                        reg_slot += slots;
                    }
                }

                // Emit the call.
                if let Some(fp) = virtual_fn_ptr {
                    self.ops.push(LIROp::CallIndirect(fp));
                } else if let Some(fp) = indirect_fn_ptr {
                    self.ops.push(LIROp::CallIndirect(fp));
                } else if let mir::Operand::Constant(c) = func {
                    if let TyKind::FnDef(def_id, substs) = c.const_.ty().kind() {
                        // Instantiate the callee's substs through the current instance's args
                        // so that abstract type parameters (e.g. `F` in `apply<F>`) become
                        // concrete (the actual closure type).
                        let mono_substs = self.tcx.instantiate_and_normalize_erasing_regions(
                            self.instance.args,
                            ty::TypingEnv::fully_monomorphized(),
                            ty::EarlyBinder::bind(*substs),
                        );
                        let inst_opt = Instance::try_resolve(self.tcx, ty::TypingEnv::fully_monomorphized(), *def_id, mono_substs)
                            .ok().flatten()
                            .or_else(|| {
                                if self.tcx.generics_of(*def_id).count() == 0 {
                                    Some(Instance::mono(self.tcx, *def_id))
                                } else {
                                    None
                                }
                            });
                        if let Some(inst) = inst_opt {
                            let sym = self.tcx.symbol_name(inst).name.to_string();
                            self.ops.push(LIROp::Call(sym));
                        } else {
                            let name = self.tcx.def_path_str(*def_id);
                            self.tcx.sess.dcx().warn(format!("unresolved generic call: {name}"));
                        }
                    } else {
                        self.ops.push(LIROp::Comment("call to non-FnDef constant".into()));
                    }
                }

                // Capture return value (x0 → destination reg or memory).
                match self.lower_place(destination, body)? {
                    PlaceLoc::Reg(dst) => {
                        if dst.0 != 0 { self.ops.push(LIROp::Move(dst, Reg(0))); }
                    }
                    PlaceLoc::Mem(ptr) => {
                        self.ops.push(LIROp::Store { src: Reg(0), base: ptr, offset: 0 });
                    }
                }
                if let Some(next) = target {
                    self.ops.push(LIROp::Jump(bb_label(*next)));
                }
            }

            mir::TerminatorKind::InlineAsm { template, operands, options: _, targets, .. } => {
                // Lower operands into x0..xN; build operand_idx→register-name table.
                let mut operand_regs: Vec<String> = vec![String::new(); operands.len()];
                let mut out_ops: Vec<LIROp> = Vec::new();

                for (idx, op) in operands.iter().enumerate() {
                    let phys = idx as u32;
                    operand_regs[idx] = format!("x{phys}");
                    match op {
                        mir::InlineAsmOperand::In { value, .. } => {
                            let src = self.lower_operand(&value, body)?;
                            if src.0 != phys { self.ops.push(LIROp::Move(Reg(phys), src)); }
                        }
                        mir::InlineAsmOperand::InOut { in_value, out_place, .. } => {
                            let src = self.lower_operand(&in_value, body)?;
                            if src.0 != phys { self.ops.push(LIROp::Move(Reg(phys), src)); }
                            if let Some(p) = out_place {
                                match self.lower_place(&p, body)? {
                                    PlaceLoc::Reg(d) => out_ops.push(LIROp::Move(d, Reg(phys))),
                                    PlaceLoc::Mem(ptr) => out_ops.push(LIROp::Store { src: Reg(phys), base: ptr, offset: 0 }),
                                }
                            }
                        }
                        mir::InlineAsmOperand::Out { place, .. } => {
                            if let Some(p) = place {
                                match self.lower_place(&p, body)? {
                                    PlaceLoc::Reg(d) => out_ops.push(LIROp::Move(d, Reg(phys))),
                                    PlaceLoc::Mem(ptr) => out_ops.push(LIROp::Store { src: Reg(phys), base: ptr, offset: 0 }),
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Build template string, substituting register names for placeholders.
                let mut lines: Vec<String> = Vec::new();
                let mut current = String::new();
                for piece in *template {
                    match piece {
                        rustc_ast::InlineAsmTemplatePiece::String(s) => {
                            for ch in s.chars() {
                                if ch == '\n' {
                                    let t = current.trim().to_string();
                                    if !t.is_empty() { lines.push(t); }
                                    current.clear();
                                } else {
                                    current.push(ch);
                                }
                            }
                        }
                        rustc_ast::InlineAsmTemplatePiece::Placeholder { operand_idx, .. } => {
                            if let Some(reg) = operand_regs.get(*operand_idx) {
                                current.push_str(reg);
                            }
                        }
                    }
                }
                let t = current.trim().to_string();
                if !t.is_empty() { lines.push(t); }

                self.ops.push(LIROp::Asm { lines });
                self.ops.extend(out_ops);
                if let Some(&next) = targets.first() {
                    self.ops.push(LIROp::Jump(bb_label(next)));
                }
            }

            _ => {
                self.ops.push(LIROp::Comment(format!("terminator: {:?}", term.kind)));
            }
        }
        Ok(())
    }

    // ── Const intrinsic lowering ───────────────────────────────────────────

    fn lower_const_intrinsic(
        &mut self,
        name: &str,
        substs: ty::GenericArgsRef<'tcx>,
        destination: &mir::Place<'tcx>,
        target: &Option<mir::BasicBlock>,
        body: &mir::Body<'tcx>,
    ) -> Result<bool, String> {
        let val: Option<u64> = match name {
            "size_of" => substs.types().next().map(|ty| self.type_size(ty)),
            "min_align_of" | "align_of" => substs.types().next().map(|ty| self.type_align(ty)),
            "needs_drop" => Some(0), // conservative: no runtime checks in our backend
            _ => None,
        };
        if let Some(val) = val {
            let tmp = self.alloc_reg();
            self.ops.push(LIROp::LoadImm(tmp, val));
            match self.lower_place(destination, body)? {
                PlaceLoc::Reg(d) => { if d != tmp { self.ops.push(LIROp::Move(d, tmp)); } }
                PlaceLoc::Mem(ptr) => { self.ops.push(LIROp::Store { src: tmp, base: ptr, offset: 0 }); }
            }
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        Ok(false)
    }

    // ── Atomic intrinsic lowering ──────────────────────────────────────────

    fn lower_atomic(
        &mut self,
        name: &str,
        args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
        destination: &mir::Place<'tcx>,
        target: &Option<mir::BasicBlock>,
        body: &mir::Body<'tcx>,
    ) -> Result<bool, String> {
        let dst_reg = match self.lower_place(destination, body)? {
            PlaceLoc::Reg(r) => r,
            PlaceLoc::Mem(p) => { let t = self.alloc_reg(); self.ops.push(LIROp::Load { dst: t, base: p, offset: 0 }); t }
        };

        // atomic_load_* : ptr → value
        if name.starts_with("atomic_load") {
            let ptr = self.lower_operand(&args[0].node, body)?;
            self.ops.push(LIROp::AtomicLoad { dst: dst_reg, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_store_* : ptr, value
        if name.starts_with("atomic_store") {
            let ptr = self.lower_operand(&args[0].node, body)?;
            let val = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicStore { src: val, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_xchg_* : ptr, val → old
        if name.starts_with("atomic_xchg") {
            let ptr = self.lower_operand(&args[0].node, body)?;
            let src = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicXchg { dst: dst_reg, src, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_cxchg_* : ptr, old, new → (old_val, ok)
        if name.starts_with("atomic_cxchg") {
            let ptr  = self.lower_operand(&args[0].node, body)?;
            let old  = self.lower_operand(&args[1].node, body)?;
            let new  = self.lower_operand(&args[2].node, body)?;
            let ok   = self.alloc_reg();
            self.ops.push(LIROp::AtomicCas { old, new, ptr, ok });
            // Result is a tuple (old_value, bool); dst_reg gets the ok flag.
            if dst_reg != ok { self.ops.push(LIROp::Move(dst_reg, ok)); }
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_xadd_* : ptr, delta → old
        if name.starts_with("atomic_xadd") {
            let ptr   = self.lower_operand(&args[0].node, body)?;
            let delta = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicFetchAdd { dst: dst_reg, delta, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_xsub_* : ptr, delta → old
        if name.starts_with("atomic_xsub") {
            let ptr   = self.lower_operand(&args[0].node, body)?;
            let delta = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicFetchSub { dst: dst_reg, delta, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_and_* : ptr, val → old
        if name.starts_with("atomic_and") {
            let ptr = self.lower_operand(&args[0].node, body)?;
            let val = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicFetchAnd { dst: dst_reg, val, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_or_* : ptr, val → old
        if name.starts_with("atomic_or") {
            let ptr = self.lower_operand(&args[0].node, body)?;
            let val = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicFetchOr { dst: dst_reg, val, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_xor_* : ptr, val → old
        if name.starts_with("atomic_xor") {
            let ptr = self.lower_operand(&args[0].node, body)?;
            let val = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicFetchXor { dst: dst_reg, val, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_nand_* : ptr, val → old
        if name.starts_with("atomic_nand") {
            let ptr = self.lower_operand(&args[0].node, body)?;
            let val = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicFetchNand { dst: dst_reg, val, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_max_* : ptr, val → old  (signed max)
        if name.starts_with("atomic_max") {
            let ptr = self.lower_operand(&args[0].node, body)?;
            let val = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicFetchMax { dst: dst_reg, val, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_min_* : ptr, val → old  (signed min)
        if name.starts_with("atomic_min") {
            let ptr = self.lower_operand(&args[0].node, body)?;
            let val = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicFetchMin { dst: dst_reg, val, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_umax_* : ptr, val → old  (unsigned max)
        if name.starts_with("atomic_umax") {
            let ptr = self.lower_operand(&args[0].node, body)?;
            let val = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicFetchUMax { dst: dst_reg, val, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_umin_* : ptr, val → old  (unsigned min)
        if name.starts_with("atomic_umin") {
            let ptr = self.lower_operand(&args[0].node, body)?;
            let val = self.lower_operand(&args[1].node, body)?;
            self.ops.push(LIROp::AtomicFetchUMin { dst: dst_reg, val, ptr });
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // atomic_fence_* and atomic_singlethreadfence_* → DMB ISH
        if name.starts_with("atomic_fence") || name.starts_with("atomic_singlethreadfence") {
            self.ops.push(LIROp::Fence);
            if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
            return Ok(true);
        }
        // Not an atomic intrinsic we handle.
        Ok(false)
    }

    // ── Misc intrinsic lowering ────────────────────────────────────────────

    fn lower_misc_intrinsic(
        &mut self,
        name: &str,
        substs: ty::GenericArgsRef<'tcx>,
        args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
        destination: &mir::Place<'tcx>,
        target: &Option<mir::BasicBlock>,
        body: &mir::Body<'tcx>,
    ) -> Result<bool, String> {
        match name {
            // ── No-ops ──────────────────────────────────────────────────────
            "forget" | "black_box" | "assume"
            | "assert_inhabited" | "assert_zero_valid" | "assert_mem_uninitialized_valid"
            | "nontemporal_store"  // treat as regular store no-op
            => {
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Terminate ───────────────────────────────────────────────────
            "abort" | "breakpoint" | "unreachable" => {
                self.ops.push(LIROp::Halt);
                return Ok(true);
            }

            // ── Bit-identity casts ──────────────────────────────────────────
            "transmute" | "transmute_unchecked" | "read_via_copy" | "write_via_move" => {
                if !args.is_empty() {
                    let src = self.lower_operand(&args[0].node, body)?;
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(d) => { if d != src { self.ops.push(LIROp::Move(d, src)); } }
                        PlaceLoc::Mem(p) => {
                            let ty  = destination.ty(&body.local_decls, self.tcx).ty;
                            let sz  = self.type_size(ty) as u8;
                            if sz > 0 && sz < 8 {
                                self.ops.push(LIROp::StoreSize { src, base: p, offset: 0, size: sz });
                            } else {
                                self.ops.push(LIROp::Store { src, base: p, offset: 0 });
                            }
                        }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Memory copy / set ───────────────────────────────────────────
            "copy_nonoverlapping" | "copy" => {
                // copy_nonoverlapping(src, dst, count): count in elements
                if args.len() >= 3 {
                    let src_ptr = self.lower_operand(&args[0].node, body)?;
                    let dst_ptr = self.lower_operand(&args[1].node, body)?;
                    let count   = self.lower_operand(&args[2].node, body)?;
                    // Scale count by element size.
                    let elem_ty = substs.types().next();
                    let elem_sz = elem_ty.map(|t| self.type_size(t)).unwrap_or(1);
                    let byte_count = if elem_sz == 1 {
                        count
                    } else {
                        let sz_reg = self.alloc_reg();
                        let bc = self.alloc_reg();
                        self.ops.push(LIROp::LoadImm(sz_reg, elem_sz));
                        self.ops.push(LIROp::Mul(bc, count, sz_reg));
                        bc
                    };
                    // Call __trident_memcpy(dst, src, byte_count).
                    self.ops.push(LIROp::Move(Reg(0), dst_ptr));
                    self.ops.push(LIROp::Move(Reg(1), src_ptr));
                    self.ops.push(LIROp::Move(Reg(2), byte_count));
                    self.ops.push(LIROp::Call("__trident_memcpy".to_string()));
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            "write_bytes" => {
                // write_bytes(dst, val, count): count in elements of T
                if args.len() >= 3 {
                    let dst_ptr = self.lower_operand(&args[0].node, body)?;
                    let val     = self.lower_operand(&args[1].node, body)?;
                    let count   = self.lower_operand(&args[2].node, body)?;
                    let elem_ty = substs.types().next();
                    let elem_sz = elem_ty.map(|t| self.type_size(t)).unwrap_or(1);
                    let byte_count = if elem_sz == 1 {
                        count
                    } else {
                        let sz_reg = self.alloc_reg();
                        let bc = self.alloc_reg();
                        self.ops.push(LIROp::LoadImm(sz_reg, elem_sz));
                        self.ops.push(LIROp::Mul(bc, count, sz_reg));
                        bc
                    };
                    self.ops.push(LIROp::Move(Reg(0), dst_ptr));
                    self.ops.push(LIROp::Move(Reg(1), val));
                    self.ops.push(LIROp::Move(Reg(2), byte_count));
                    self.ops.push(LIROp::Call("__trident_memset".to_string()));
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Pointer arithmetic ──────────────────────────────────────────
            "offset" | "arith_offset" => {
                if args.len() >= 2 {
                    let ptr = self.lower_operand(&args[0].node, body)?;
                    let off = self.lower_operand(&args[1].node, body)?;
                    let elem_ty = substs.types().next();
                    let elem_sz = elem_ty.map(|t| self.type_size(t)).unwrap_or(1);
                    let result = self.alloc_reg();
                    if elem_sz == 1 {
                        self.ops.push(LIROp::Add(result, ptr, off));
                    } else {
                        let sz_reg   = self.alloc_reg();
                        let byte_off = self.alloc_reg();
                        self.ops.push(LIROp::LoadImm(sz_reg, elem_sz));
                        self.ops.push(LIROp::Mul(byte_off, off, sz_reg));
                        self.ops.push(LIROp::Add(result, ptr, byte_off));
                    }
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(d) => { if d != result { self.ops.push(LIROp::Move(d, result)); } }
                        PlaceLoc::Mem(p) => { self.ops.push(LIROp::Store { src: result, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Wrapping arithmetic (identical to regular in our backend) ───
            "wrapping_add" => {
                if args.len() >= 2 {
                    let a = self.lower_operand(&args[0].node, body)?;
                    let b = self.lower_operand(&args[1].node, body)?;
                    let d = self.alloc_reg();
                    self.ops.push(LIROp::Add(d, a, b));
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }
            "wrapping_sub" => {
                if args.len() >= 2 {
                    let a = self.lower_operand(&args[0].node, body)?;
                    let b = self.lower_operand(&args[1].node, body)?;
                    let d = self.alloc_reg();
                    self.ops.push(LIROp::Sub(d, a, b));
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }
            "wrapping_mul" => {
                if args.len() >= 2 {
                    let a = self.lower_operand(&args[0].node, body)?;
                    let b = self.lower_operand(&args[1].node, body)?;
                    let d = self.alloc_reg();
                    self.ops.push(LIROp::Mul(d, a, b));
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Bit ops ─────────────────────────────────────────────────────
            "rotate_left" => {
                // ROL by n = ROR by (64-n)
                if args.len() >= 2 {
                    let val = self.lower_operand(&args[0].node, body)?;
                    let rot = self.lower_operand(&args[1].node, body)?;
                    let sixty4 = self.alloc_reg();
                    let rot_r  = self.alloc_reg();
                    let d      = self.alloc_reg();
                    self.ops.push(LIROp::LoadImm(sixty4, 64));
                    self.ops.push(LIROp::Sub(rot_r, sixty4, rot));
                    // ROR via: (val >> rot_r) | (val << rot)
                    let hi = self.alloc_reg();
                    let lo = self.alloc_reg();
                    self.ops.push(LIROp::Shr(hi, val, rot_r));
                    self.ops.push(LIROp::Shl(lo, val, rot));
                    self.ops.push(LIROp::Or(d, hi, lo));
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }
            "rotate_right" => {
                if args.len() >= 2 {
                    let val = self.lower_operand(&args[0].node, body)?;
                    let rot = self.lower_operand(&args[1].node, body)?;
                    let sixty4 = self.alloc_reg();
                    let rot_l  = self.alloc_reg();
                    let d      = self.alloc_reg();
                    self.ops.push(LIROp::LoadImm(sixty4, 64));
                    self.ops.push(LIROp::Sub(rot_l, sixty4, rot));
                    let hi = self.alloc_reg();
                    let lo = self.alloc_reg();
                    self.ops.push(LIROp::Shr(hi, val, rot));
                    self.ops.push(LIROp::Shl(lo, val, rot_l));
                    self.ops.push(LIROp::Or(d, hi, lo));
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Count bits ──────────────────────────────────────────────────
            "ctlz" | "ctlz_nonzero" => {
                if !args.is_empty() {
                    let val = self.lower_operand(&args[0].node, body)?;
                    let d   = self.alloc_reg();
                    self.ops.push(LIROp::Move(Reg(8), val));
                    self.ops.push(LIROp::Asm { lines: vec!["clz x8, x8".to_string()] });
                    self.ops.push(LIROp::Move(d, Reg(8)));
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }
            "cttz" | "cttz_nonzero" => {
                if !args.is_empty() {
                    let val = self.lower_operand(&args[0].node, body)?;
                    let d   = self.alloc_reg();
                    self.ops.push(LIROp::Move(Reg(8), val));
                    // CTZ via RBIT then CLZ
                    self.ops.push(LIROp::Asm { lines: vec![
                        "rbit x8, x8".to_string(),
                        "clz x8, x8".to_string(),
                    ]});
                    self.ops.push(LIROp::Move(d, Reg(8)));
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }
            "ctpop" => {
                if !args.is_empty() {
                    let val = self.lower_operand(&args[0].node, body)?;
                    let d   = self.alloc_reg();
                    self.ops.push(LIROp::Move(Reg(8), val));
                    self.ops.push(LIROp::Asm { lines: vec![
                        "fmov d0, x8".to_string(),
                        "cnt v0.8b, v0.8b".to_string(),
                        "addv b0, v0.8b".to_string(),
                        "fmov w8, s0".to_string(),
                    ]});
                    self.ops.push(LIROp::Move(d, Reg(8)));
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Misc size/align ─────────────────────────────────────────────
            "min_align_of_val" | "align_of_val" => {
                let val = substs.types().next().map(|t| self.type_align(t)).unwrap_or(1);
                let tmp = self.alloc_reg();
                self.ops.push(LIROp::LoadImm(tmp, val));
                match self.lower_place(destination, body)? {
                    PlaceLoc::Reg(d) => { if d != tmp { self.ops.push(LIROp::Move(d, tmp)); } }
                    PlaceLoc::Mem(p) => { self.ops.push(LIROp::Store { src: tmp, base: p, offset: 0 }); }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }
            "size_of_val" => {
                let val = substs.types().next().map(|t| self.type_size(t)).unwrap_or(0);
                let tmp = self.alloc_reg();
                self.ops.push(LIROp::LoadImm(tmp, val));
                match self.lower_place(destination, body)? {
                    PlaceLoc::Reg(d) => { if d != tmp { self.ops.push(LIROp::Move(d, tmp)); } }
                    PlaceLoc::Mem(p) => { self.ops.push(LIROp::Store { src: tmp, base: p, offset: 0 }); }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Volatile / unordered (treat as non-volatile for now) ────────
            "volatile_load" | "unaligned_volatile_load" => {
                if !args.is_empty() {
                    let ptr = self.lower_operand(&args[0].node, body)?;
                    let ty  = destination.ty(&body.local_decls, self.tcx).ty;
                    let sz  = self.type_size(ty) as u8;
                    let d   = self.alloc_reg();
                    if sz > 0 && sz < 8 {
                        self.ops.push(LIROp::LoadSize { dst: d, base: ptr, offset: 0, size: sz });
                    } else {
                        self.ops.push(LIROp::Load { dst: d, base: ptr, offset: 0 });
                    }
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }
            "volatile_store" | "unaligned_volatile_store" => {
                if args.len() >= 2 {
                    let ptr = self.lower_operand(&args[0].node, body)?;
                    let val = self.lower_operand(&args[1].node, body)?;
                    self.ops.push(LIROp::Store { src: val, base: ptr, offset: 0 });
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Discriminant ────────────────────────────────────────────────
            "discriminant_value" => {
                if !args.is_empty() {
                    let ptr = self.lower_operand(&args[0].node, body)?;
                    let d   = self.alloc_reg();
                    self.ops.push(LIROp::Load { dst: d, base: ptr, offset: 0 });
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Unchecked / exact arithmetic (same machine semantics in our backend) ──
            "unchecked_add" | "unchecked_sub" | "unchecked_mul"
            | "unchecked_div" | "unchecked_rem"
            | "unchecked_shl" | "unchecked_shr"
            | "exact_div" => {
                if args.len() >= 2 {
                    let a = self.lower_operand(&args[0].node, body)?;
                    let b = self.lower_operand(&args[1].node, body)?;
                    let d = self.alloc_reg();
                    let signed = substs.types().next()
                        .map(|t| matches!(t.kind(), TyKind::Int(_)))
                        .unwrap_or(false);
                    let insn = match name {
                        "unchecked_add"          => LIROp::Add(d, a, b),
                        "unchecked_sub"          => LIROp::Sub(d, a, b),
                        "unchecked_mul"          => LIROp::Mul(d, a, b),
                        "unchecked_div" | "exact_div"
                                                 => if signed { LIROp::SDiv(d,a,b) } else { LIROp::Div(d,a,b) },
                        "unchecked_rem"          => LIROp::Rem(d, a, b),
                        "unchecked_shl"          => LIROp::Shl(d, a, b),
                        "unchecked_shr"          => if signed { LIROp::Sar(d,a,b) } else { LIROp::Shr(d,a,b) },
                        _                        => unreachable!(),
                    };
                    self.ops.push(insn);
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Saturating arithmetic (wrapping in our backend; overflow is UB in safe code) ──
            "saturating_add" | "saturating_sub" => {
                if args.len() >= 2 {
                    let a = self.lower_operand(&args[0].node, body)?;
                    let b = self.lower_operand(&args[1].node, body)?;
                    let d = self.alloc_reg();
                    let insn = if name == "saturating_add" { LIROp::Add(d,a,b) } else { LIROp::Sub(d,a,b) };
                    self.ops.push(insn);
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            // ── Pointer cast ────────────────────────────────────────────────
            "ptr_guaranteed_cmp" => {
                if args.len() >= 2 {
                    let a = self.lower_operand(&args[0].node, body)?;
                    let b = self.lower_operand(&args[1].node, body)?;
                    let d = self.alloc_reg();
                    self.ops.push(LIROp::Eq(d, a, b));
                    match self.lower_place(destination, body)? {
                        PlaceLoc::Reg(dst) => { if dst != d { self.ops.push(LIROp::Move(dst, d)); } }
                        PlaceLoc::Mem(p)   => { self.ops.push(LIROp::Store { src: d, base: p, offset: 0 }); }
                    }
                }
                if let Some(bb) = target { self.ops.push(LIROp::Jump(bb_label(*bb))); }
                return Ok(true);
            }

            _ => {}
        }
        Ok(false)
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn bb_label(bb: mir::BasicBlock) -> Label {
    Label::new(format!("bb{}", bb.index()))
}

fn int_bits(ty: ty::Ty<'_>) -> u32 {
    match ty.kind() {
        TyKind::Int(ity) => match ity {
            ty::IntTy::I8    => 8,  ty::IntTy::I16   => 16,
            ty::IntTy::I32   => 32, ty::IntTy::I64   => 64,
            ty::IntTy::I128  => 128, ty::IntTy::Isize => 64,
        },
        TyKind::Uint(uty) => match uty {
            ty::UintTy::U8   => 8,  ty::UintTy::U16  => 16,
            ty::UintTy::U32  => 32, ty::UintTy::U64  => 64,
            ty::UintTy::U128 => 128, ty::UintTy::Usize => 64,
        },
        TyKind::Bool  => 1,
        TyKind::Char  => 32,
        _ => 64,
    }
}
