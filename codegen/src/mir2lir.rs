//! MIR → LIR lowering. Both IRs are flat 3-address; this is a near-1:1 mapping.

use std::collections::HashMap;

use rustc_middle::mir;
use rustc_middle::mir::ConstValue;
use rustc_middle::mir::interpret::{GlobalAlloc, Scalar};
use rustc_middle::ty::{self, Instance, TyCtxt, TyKind};

use crate::lir::{Label, LIROp, Reg};

pub struct StaticData {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Whether a MIR place lives in a register or behind a memory pointer.
enum PlaceLoc {
    Reg(Reg),   // value is directly in the virtual register
    Mem(Reg),   // value lives at the 64-bit address in this register
}

pub struct MirToLir<'tcx> {
    tcx:      TyCtxt<'tcx>,
    next_vreg: u32,
    ops:       Vec<LIROp>,
    locals:    HashMap<mir::Local, Reg>,
    statics:   Vec<StaticData>,
}

impl<'tcx> MirToLir<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        Self { tcx, next_vreg: 0, ops: Vec::new(), locals: HashMap::new(), statics: Vec::new() }
    }

    pub fn take_statics(&mut self) -> Vec<StaticData> { std::mem::take(&mut self.statics) }

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
        self.ops.clear();
        self.locals.clear();
        self.next_vreg = 0;

        let fn_name = self.tcx.symbol_name(instance).name.to_string();
        self.ops.push(LIROp::FnStart(fn_name));

        // ABI: local 0 = return place, 1..=arg_count = args → x0..xN
        let arg_count = body.arg_count;
        for i in 1..=arg_count {
            let local = mir::Local::from_usize(i);
            self.locals.insert(local, Reg((i - 1) as u32));
        }
        self.locals.insert(mir::Local::from_usize(0), Reg(0));
        self.next_vreg = arg_count.max(15) as u32;

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
            _ => {
                self.ops.push(LIROp::Comment(format!("stmt: {:?}", stmt.kind)));
            }
        }
        Ok(())
    }

    // ── Place lowering ─────────────────────────────────────────────────────

    fn lower_place(
        &mut self,
        place: &mir::Place<'tcx>,
        body: &mir::Body<'tcx>,
    ) -> Result<PlaceLoc, String> {
        if place.projection.is_empty() {
            return Ok(PlaceLoc::Reg(self.local_reg(place.local)));
        }

        let mut cur_reg = self.local_reg(place.local);
        let mut cur_ty  = body.local_decls[place.local].ty;
        let mut is_mem  = false;

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
                    is_mem = true;
                    // No instruction: cur_reg already IS the address.
                }
                mir::PlaceElem::Field(field_idx, field_ty) => {
                    let offset = self.field_byte_offset(cur_ty, field_idx.as_usize());
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
                mir::PlaceElem::Downcast(..) => { /* type-only narrowing, no address change */ }
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
                let dst = self.alloc_reg();
                self.ops.push(LIROp::Load { dst, base: ptr, offset: 0 });
                Ok(dst)
            }
        }
    }

    // ── Layout helpers ─────────────────────────────────────────────────────

    fn field_byte_offset(&self, ty: ty::Ty<'tcx>, field_idx: usize) -> u64 {
        let env = ty::TypingEnv::fully_monomorphized();
        self.tcx.layout_of(env.as_query_input(ty))
            .map(|l| l.fields.offset(field_idx).bytes())
            .unwrap_or(0)
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

    fn elem_type(&self, ty: ty::Ty<'tcx>) -> Option<ty::Ty<'tcx>> {
        match ty.kind() {
            TyKind::Array(et, _) | TyKind::Slice(et) => Some(*et),
            _ => None,
        }
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
                let s = self.lower_operand(operand, body)?;
                let insn = match op {
                    mir::UnOp::Not => LIROp::Not(dst, s),
                    mir::UnOp::Neg => LIROp::Neg(dst, s),
                    mir::UnOp::PtrMetadata => {
                        // fat pointer metadata (len or vtable ptr) is the high word.
                        // We don't track fat pointers, emit zero.
                        LIROp::LoadImm(dst, 0)
                    }
                };
                self.ops.push(insn);
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
                    // Taking address of a local — it would need to be stack-allocated.
                    // Phase 3 best-effort: treat the local's register value as the address.
                    let base = self.local_reg(place.local);
                    if dst != base { self.ops.push(LIROp::Move(dst, base)); }
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
                        // General case: best-effort zero.
                        self.ops.push(LIROp::LoadImm(dst, 0));
                    }
                }
            }

            mir::Rvalue::Discriminant(place) => {
                // Read the discriminant of an enum. For C-like enums it's at offset 0.
                let ptr = self.lower_place_to_reg(place, body)?;
                self.ops.push(LIROp::Load { dst, base: ptr, offset: 0 });
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
            // All pointer/transmute/fn-ptr casts: same bit pattern, just move.
            _ => {
                if dst != src { self.ops.push(LIROp::Move(dst, src)); }
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
            let bytes = alloc.inner().inspect_with_uninit_and_ptr_outside_interpreter(0..alloc.inner().len()).to_vec();
            self.statics.push(StaticData { name: name.clone(), bytes });
            return Some(name);
        }
        None
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

            // Drop: with panic=abort there's no unwind. Emit the drop call if we know the
            // symbol, then jump to target.
            mir::TerminatorKind::Drop { place: _, target, .. } => {
                // Phase 3: skip drop glue — just jump to target.
                // (Correct for types without custom Drop or for panic=abort flows.)
                self.ops.push(LIROp::Jump(bb_label(*target)));
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
                            // Atomics are handled before argument setup.
                            if let Some(intr) = self.tcx.intrinsic(*def_id) {
                                if self.lower_atomic(intr.name.as_str(), args, destination, target, body)? {
                                    return Ok(());
                                }
                            }
                        }
                    }
                }

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
                for (i, arg) in args.iter().enumerate() {
                    let src = self.lower_operand(&arg.node, body)?;
                    if src.0 != i as u32 {
                        self.ops.push(LIROp::Move(Reg(i as u32), src));
                    }
                }

                // Emit the call.
                if let Some(fp) = indirect_fn_ptr {
                    self.ops.push(LIROp::CallIndirect(fp));
                } else if let mir::Operand::Constant(c) = func {
                    if let TyKind::FnDef(def_id, substs) = c.const_.ty().kind() {
                        let inst = Instance::try_resolve(self.tcx, ty::TypingEnv::fully_monomorphized(), *def_id, substs)
                            .ok().flatten()
                            .unwrap_or_else(|| Instance::mono(self.tcx, *def_id));
                        let sym = self.tcx.symbol_name(inst).name.to_string();
                        self.ops.push(LIROp::Call(sym));
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

            mir::TerminatorKind::InlineAsm { template, operands: _, options: _, targets, .. } => {
                let mut lines: Vec<String> = Vec::new();
                for piece in *template {
                    match piece {
                        rustc_ast::InlineAsmTemplatePiece::String(s) => {
                            for line in s.split('\n') {
                                let t = line.trim();
                                if !t.is_empty() { lines.push(t.to_string()); }
                            }
                        }
                        rustc_ast::InlineAsmTemplatePiece::Placeholder { .. } => {
                            lines.push("nop".to_string()); // placeholder not yet expanded
                        }
                    }
                }
                self.ops.push(LIROp::Asm { lines });
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
        // Not an atomic intrinsic we handle.
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
