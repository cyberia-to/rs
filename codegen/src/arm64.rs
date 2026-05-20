//! ARM64 LIR lowering backend.
//!
//! Register mapping:
//!   Reg(0..7)   → x0-x7   (ABI args/return)
//!   Reg(8..14)  → x9-x15  (caller-saved scratch; x8 = indirect result, skipped)
//!   Reg(15..23) → x19-x27 (callee-saved)
//!   Reg(24+)    → FP-relative spill slots; x16/x17 used as scratch for spill loads

use std::collections::HashMap;

use crate::lir::{LIROp, Reg, Label};
use super::encoders as enc;

const FP: u8 = 29;
const LR: u8 = 30;
const SP: u8 = 31;
const SCRATCH0: u8 = 16;  // x16 — spill scratch (not callee-saved)
const SCRATCH1: u8 = 17;  // x17 — spill scratch
const CALLEE_FIRST: u8 = 19;
const CALLEE_LAST: u8 = 27;

fn phys_direct(vr: Reg) -> Option<u8> {
    match vr.0 {
        0..=7   => Some(vr.0 as u8),
        8..=14  => Some(vr.0 as u8 + 1),  // x9-x15
        15..=23 => Some(vr.0 as u8 + 4),  // x19-x27
        _       => None,
    }
}

/// Relocation for a BL (Branch26) to an external symbol.
#[derive(Debug, Clone)]
pub struct CallReloc {
    pub offset: usize,
    pub symbol: String,
}

/// Relocation for an ADRP+ADD pair referencing a data symbol.
#[derive(Debug, Clone)]
pub struct DataReloc {
    pub adrp_offset: usize,
    pub add_offset:  usize,
    pub symbol:      String,
}

pub struct Arm64Backend {
    call_relocs:  Vec<CallReloc>,
    data_relocs:  Vec<DataReloc>,
    fn_offsets:   HashMap<String, usize>,  // symbol → byte offset in emitted code
}

impl Arm64Backend {
    pub fn new() -> Self {
        Self { call_relocs: Vec::new(), data_relocs: Vec::new(), fn_offsets: HashMap::new() }
    }
    pub fn call_relocs(&self) -> &[CallReloc] { &self.call_relocs }
    pub fn data_relocs(&self) -> &[DataReloc] { &self.data_relocs }
    pub fn fn_offsets(&self)  -> &HashMap<String, usize> { &self.fn_offsets }

    pub fn lower(&mut self, ops: &[LIROp]) -> Vec<u8> {
        let mut ctx = Ctx::new();
        ctx.lower_all(ops);
        self.call_relocs = ctx.call_relocs;
        self.data_relocs = ctx.data_relocs;
        self.fn_offsets  = ctx.fn_offsets;
        ctx.code
    }
}

// ── Lowering context ────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum PatchKind { B26, B19 }

struct Ctx {
    code:        Vec<u8>,
    labels:      HashMap<String, usize>,
    patches:     Vec<(usize, String, PatchKind)>,
    call_relocs: Vec<CallReloc>,
    data_relocs: Vec<DataReloc>,
    fn_offsets:  HashMap<String, usize>,
    callee_used: Vec<u8>,
    // Spill: Reg(24+) → FP-relative slot index (slot 0 = [fp-8], slot 1 = [fp-16], ...)
    spill_map:        HashMap<u32, u32>,
    spill_count:      u32,  // slots actually used during lowering
    spill_alloc_bytes: u16, // bytes actually subtracted from SP in prologue
}

impl Ctx {
    fn new() -> Self {
        Self {
            code:        Vec::with_capacity(512),
            labels:      HashMap::new(),
            patches:     Vec::new(),
            call_relocs: Vec::new(),
            data_relocs: Vec::new(),
            fn_offsets:  HashMap::new(),
            callee_used:       Vec::new(),
            spill_map:         HashMap::new(),
            spill_count:       0,
            spill_alloc_bytes: 0,
        }
    }

    fn emit(&mut self, insn: u32) { self.code.extend_from_slice(&insn.to_le_bytes()); }
    fn emit_many(&mut self, insns: Vec<u32>) { for i in insns { self.emit(i); } }
    fn offset(&self) -> usize { self.code.len() }

    fn def_label(&mut self, name: &str) { self.labels.insert(name.to_string(), self.offset()); }

    fn emit_b_placeholder(&mut self, target: &str) {
        let off = self.offset();
        self.patches.push((off, target.to_string(), PatchKind::B26));
        self.emit(0x14000000);
    }

    fn emit_cbnz_placeholder(&mut self, rn: u8, target: &str) {
        let off = self.offset();
        self.patches.push((off, target.to_string(), PatchKind::B19));
        self.emit(0xB5000000 | (rn as u32));
    }

    fn patch_branches(&mut self) {
        for (patch_off, label, kind) in self.patches.drain(..).collect::<Vec<_>>() {
            let target_off = *self.labels.get(&label)
                .unwrap_or_else(|| panic!("undefined label: {label}"));
            let delta = (target_off as i64 - patch_off as i64) / 4;
            let word = u32::from_le_bytes(self.code[patch_off..patch_off+4].try_into().unwrap());
            let patched = match kind {
                PatchKind::B26 => (word & 0xFC00_0000) | ((delta as u32) & 0x03FF_FFFF),
                PatchKind::B19 => (word & !0x00FFFFE0) | (((delta as u32) & 0x7FFFF) << 5),
            };
            self.code[patch_off..patch_off+4].copy_from_slice(&patched.to_le_bytes());
        }
    }

    // ── Register allocation ──────────────────────────────────────────

    fn spill_slot(&mut self, vr_idx: u32) -> u32 {
        if let Some(&slot) = self.spill_map.get(&vr_idx) { return slot; }
        let slot = self.spill_count;
        self.spill_count += 1;
        self.spill_map.insert(vr_idx, slot);
        slot
    }

    // Load vr into scratch register `scratch`, emit the load instruction.
    fn load_spill(&mut self, vr: Reg, scratch: u8) {
        let slot = self.spill_slot(vr.0);
        let offset = -((slot as i32 + 1) * 8);  // FP-relative: slot0=[fp-8]
        if offset >= -256 {
            self.emit(enc::ldur(scratch, FP, offset as i16));
        } else {
            self.emit_many(enc::mov_imm64(scratch, offset as u64));
            self.emit(enc::add(scratch, FP, scratch));
            self.emit(enc::ldr_imm(scratch, scratch, 0));
        }
    }

    // Store `scratch` register into vr's spill slot.
    fn store_spill(&mut self, vr: Reg, scratch: u8) {
        let slot = self.spill_slot(vr.0);
        let offset = -((slot as i32 + 1) * 8);
        if offset >= -256 {
            self.emit(enc::stur(scratch, FP, offset as i16));
        } else {
            self.emit_many(enc::mov_imm64(SCRATCH1, offset as u64));
            self.emit(enc::add(SCRATCH1, FP, SCRATCH1));
            self.emit(enc::str_imm(scratch, SCRATCH1, 0));
        }
    }

    // Get a physical register for reading. Returns SCRATCH0 if spilled (and emits load).
    fn r_read(&mut self, vr: Reg) -> u8 {
        match phys_direct(vr) {
            Some(p) => p,
            None => { self.load_spill(vr, SCRATCH0); SCRATCH0 }
        }
    }

    // Get a physical register for writing. Returns SCRATCH0 if spilled
    // (caller must call commit_write after emitting the instruction).
    fn r_write(&mut self, vr: Reg) -> u8 {
        match phys_direct(vr) { Some(p) => p, None => SCRATCH0 }
    }

    // After writing SCRATCH0 for a spilled vr, commit to memory.
    fn commit_write(&mut self, vr: Reg) {
        if phys_direct(vr).is_none() { self.store_spill(vr, SCRATCH0); }
    }

    // Read two source operands without clobbering each other.
    fn r2_read(&mut self, a: Reg, b: Reg) -> (u8, u8) {
        let pa = match phys_direct(a) {
            Some(p) => p,
            None => { self.load_spill(a, SCRATCH0); SCRATCH0 }
        };
        let pb = match phys_direct(b) {
            Some(p) => p,
            None => { self.load_spill(b, SCRATCH1); SCRATCH1 }
        };
        (pa, pb)
    }

    // ── Callee-save scanning ─────────────────────────────────────────

    fn collect_regs(op: &LIROp) -> Vec<Reg> {
        match op {
            LIROp::LoadImm(d,_) | LIROp::Neg(d,_) | LIROp::Not(d,_) => vec![*d],
            LIROp::Move(d,s) => vec![*d,*s],
            LIROp::LoadAddr{dst,..} => vec![*dst],
            LIROp::Add(d,a,b)|LIROp::Sub(d,a,b)|LIROp::Mul(d,a,b)
            |LIROp::Div(d,a,b)|LIROp::SDiv(d,a,b)|LIROp::Rem(d,a,b)
            |LIROp::And(d,a,b)|LIROp::Or(d,a,b)|LIROp::Xor(d,a,b)
            |LIROp::Shl(d,a,b)|LIROp::Shr(d,a,b)|LIROp::Sar(d,a,b)
            |LIROp::Eq(d,a,b)|LIROp::Ne(d,a,b)
            |LIROp::Lt(d,a,b)|LIROp::Le(d,a,b)|LIROp::Gt(d,a,b)|LIROp::Ge(d,a,b)
            |LIROp::SLt(d,a,b)|LIROp::SLe(d,a,b)|LIROp::SGt(d,a,b)|LIROp::SGe(d,a,b)
            => vec![*d,*a,*b],
            LIROp::ZeroExt{dst,src,..}|LIROp::SignExt{dst,src,..} => vec![*dst,*src],
            LIROp::Load{dst,base,..} => vec![*dst,*base],
            LIROp::Store{src,base,..} => vec![*src,*base],
            LIROp::LoadSize{dst,base,..} => vec![*dst,*base],
            LIROp::StoreSize{src,base,..} => vec![*src,*base],
            LIROp::AtomicLoad{dst,ptr}|LIROp::AtomicStore{src:dst,ptr} => vec![*dst,*ptr],
            LIROp::AtomicXchg{dst,src,ptr}
            |LIROp::AtomicFetchAdd{dst,delta:src,ptr}
            |LIROp::AtomicFetchSub{dst,delta:src,ptr} => vec![*dst,*src,*ptr],
            LIROp::AtomicCas{old,new,ptr,ok} => vec![*old,*new,*ptr,*ok],
            LIROp::Branch{cond,..} => vec![*cond],
            LIROp::CallIndirect(r) => vec![*r],
            _ => vec![],
        }
    }

    fn scan_callee(ops: &[LIROp]) -> Vec<u8> {
        let mut used = std::collections::BTreeSet::new();
        for op in ops {
            for reg in Self::collect_regs(op) {
                if let Some(p) = phys_direct(reg) {
                    if (CALLEE_FIRST..=CALLEE_LAST).contains(&p) { used.insert(p); }
                }
            }
        }
        used.into_iter().collect()
    }

    fn count_spills(ops: &[LIROp]) -> u32 {
        let mut max_spill: i64 = -1;
        for op in ops {
            for reg in Self::collect_regs(op) {
                if reg.0 >= 24 { max_spill = max_spill.max(reg.0 as i64); }
            }
        }
        if max_spill < 0 { 0 } else { (max_spill - 23) as u32 }
    }

    // ── Prologue / epilogue ──────────────────────────────────────────

    fn emit_prologue(&mut self, callee: Vec<u8>, spill_count: u32) {
        // Save FP+LR
        self.emit(enc::stp_pre(FP, LR, SP, -2));
        self.emit(enc::add_imm(FP, SP, 0));  // MOV x29, sp

        // Save callee-saved regs
        let pairs: Vec<Vec<u8>> = callee.chunks(2).map(|c| c.to_vec()).collect();
        for chunk in &pairs {
            if chunk.len() == 2 {
                self.emit(enc::stp_pre(chunk[0], chunk[1], SP, -2));
            } else {
                // stp with Rt1==Rt2 is CONSTRAINED UNPREDICTABLE; use str instead
                self.emit(enc::sub_imm(SP, SP, 16));
                self.emit(enc::str_imm(chunk[0], SP, 0));
            }
        }

        // Allocate spill area below FP frame: round up to 16-byte alignment
        let spill_bytes = if spill_count > 0 {
            let bytes = ((spill_count as u16 * 8) + 15) & !15;
            self.emit(enc::sub_imm(SP, SP, bytes));
            bytes
        } else { 0 };

        self.callee_used       = callee;
        self.spill_count       = 0;
        self.spill_alloc_bytes = spill_bytes;
        self.spill_map.clear();
    }

    fn emit_epilogue(&mut self) {
        if self.spill_alloc_bytes > 0 {
            self.emit(enc::add_imm(SP, SP, self.spill_alloc_bytes));
        }
        let pairs: Vec<Vec<u8>> = self.callee_used.chunks(2)
            .map(|c| c.to_vec()).collect::<Vec<_>>();
        for chunk in pairs.into_iter().rev() {
            if chunk.len() == 2 {
                self.emit(enc::ldp_post(chunk[0], chunk[1], SP, 2));
            } else {
                // ldp with Rt1==Rt2 is CONSTRAINED UNPREDICTABLE; use ldr instead
                self.emit(enc::ldr_imm(chunk[0], SP, 0));
                self.emit(enc::add_imm(SP, SP, 16));
            }
        }
        self.emit(enc::ldp_post(FP, LR, SP, 2));
        self.emit(enc::ret());
    }

    // ── Main lowering ────────────────────────────────────────────────

    fn lower_all(&mut self, ops: &[LIROp]) {
        let callee      = Self::scan_callee(ops);
        let spill_count = Self::count_spills(ops);

        for op in ops {
            match op {
                LIROp::Comment(_) => {}

                LIROp::FnStart(name) => {
                    self.fn_offsets.insert(name.clone(), self.offset());
                    self.emit_prologue(callee.clone(), spill_count);
                }

                LIROp::FnEnd => {
                    self.emit_epilogue();
                    self.patch_branches();
                }

                LIROp::LabelDef(Label(name)) => self.def_label(name),

                LIROp::Return => self.emit_epilogue(),

                LIROp::Halt => {
                    // exit(0): x16=1, x0=0, svc 0x80
                    self.emit_many(enc::mov_imm64(0, 0));
                    self.emit_many(enc::mov_imm64(16, 1));
                    self.emit(enc::svc(0x80));
                }

                LIROp::Jump(Label(name)) => self.emit_b_placeholder(name),

                LIROp::Branch { cond, if_true, if_false } => {
                    let rn = self.r_read(*cond);
                    self.emit_cbnz_placeholder(rn, &if_true.0);
                    self.emit_b_placeholder(&if_false.0);
                }

                LIROp::Call(name) => {
                    let off = self.offset();
                    self.emit(enc::bl_placeholder());
                    self.call_relocs.push(CallReloc { offset: off, symbol: name.clone() });
                }

                LIROp::CallIndirect(reg) => {
                    let rn = self.r_read(*reg);
                    self.emit(enc::blr(rn));
                }

                LIROp::LoadImm(dst, val) => {
                    let d = self.r_write(*dst);
                    self.emit_many(enc::mov_imm64(d, *val));
                    self.commit_write(*dst);
                }

                LIROp::Move(dst, src) => {
                    let s = self.r_read(*src);
                    let d = self.r_write(*dst);
                    if d != s { self.emit(enc::mov_reg(d, s)); }
                    self.commit_write(*dst);
                }

                LIROp::LoadAddr { dst, symbol } => {
                    let d = self.r_write(*dst);
                    let adrp_off = self.offset();
                    self.emit(enc::adrp(d, 0));
                    let add_off = self.offset();
                    self.emit(0x91000000u32 | (d as u32) << 5 | (d as u32));
                    self.data_relocs.push(DataReloc {
                        adrp_offset: adrp_off,
                        add_offset: add_off,
                        symbol: symbol.clone(),
                    });
                    self.commit_write(*dst);
                }

                // ── Arithmetic ───────────────────────────────────────

                LIROp::Add(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::add(pd,pa,pb)); self.commit_write(*d);
                }
                LIROp::Sub(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::sub(pd,pa,pb)); self.commit_write(*d);
                }
                LIROp::Mul(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::mul(pd,pa,pb)); self.commit_write(*d);
                }
                LIROp::Div(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::udiv(pd,pa,pb)); self.commit_write(*d);
                }
                LIROp::SDiv(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::sdiv(pd,pa,pb)); self.commit_write(*d);
                }
                LIROp::Rem(d,a,b) => {
                    // udiv tmp,a,b; msub d,tmp,b,a  →  d = a - (a/b)*b
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::udiv(SCRATCH1,pa,pb));
                    self.emit(enc::msub(pd,SCRATCH1,pb,pa));
                    self.commit_write(*d);
                }
                LIROp::Neg(d,s) => {
                    let ps = self.r_read(*s); let pd = self.r_write(*d);
                    self.emit(enc::neg(pd,ps)); self.commit_write(*d);
                }
                LIROp::Not(d,s) => {
                    let ps = self.r_read(*s); let pd = self.r_write(*d);
                    self.emit(enc::mvn(pd,ps)); self.commit_write(*d);
                }

                // ── Bitwise ──────────────────────────────────────────

                LIROp::And(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::and(pd,pa,pb)); self.commit_write(*d);
                }
                LIROp::Or(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::orr(pd,pa,pb)); self.commit_write(*d);
                }
                LIROp::Xor(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::eor(pd,pa,pb)); self.commit_write(*d);
                }
                LIROp::Shl(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::lsl_reg(pd,pa,pb)); self.commit_write(*d);
                }
                LIROp::Shr(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::lsr_reg(pd,pa,pb)); self.commit_write(*d);
                }
                LIROp::Sar(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::asr_reg(pd,pa,pb)); self.commit_write(*d);
                }

                // ── Comparisons ──────────────────────────────────────
                // ARM64 condition codes: EQ=0, NE=1, CS=2, CC=3, MI=4, PL=5,
                // VS=6, VC=7, HI=8, LS=9, GE=10, LT=11, GT=12, LE=13

                LIROp::Eq(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::cmp(pa,pb)); self.emit(enc::cset(pd,0)); self.commit_write(*d);
                }
                LIROp::Ne(d,a,b) => {
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::cmp(pa,pb)); self.emit(enc::cset(pd,1)); self.commit_write(*d);
                }
                LIROp::Lt(d,a,b) => {
                    // unsigned < : CC(LO)=3
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::cmp(pa,pb)); self.emit(enc::cset(pd,3)); self.commit_write(*d);
                }
                LIROp::Le(d,a,b) => {
                    // unsigned <= : LS=9
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::cmp(pa,pb)); self.emit(enc::cset(pd,9)); self.commit_write(*d);
                }
                LIROp::Gt(d,a,b) => {
                    // unsigned > : HI=8
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::cmp(pa,pb)); self.emit(enc::cset(pd,8)); self.commit_write(*d);
                }
                LIROp::Ge(d,a,b) => {
                    // unsigned >= : CS=2
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::cmp(pa,pb)); self.emit(enc::cset(pd,2)); self.commit_write(*d);
                }
                LIROp::SLt(d,a,b) => {
                    // signed < : LT=11
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::cmp(pa,pb)); self.emit(enc::cset(pd,11)); self.commit_write(*d);
                }
                LIROp::SLe(d,a,b) => {
                    // signed <= : LE=13
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::cmp(pa,pb)); self.emit(enc::cset(pd,13)); self.commit_write(*d);
                }
                LIROp::SGt(d,a,b) => {
                    // signed > : GT=12
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::cmp(pa,pb)); self.emit(enc::cset(pd,12)); self.commit_write(*d);
                }
                LIROp::SGe(d,a,b) => {
                    // signed >= : GE=10
                    let (pa,pb) = self.r2_read(*a,*b); let pd = self.r_write(*d);
                    self.emit(enc::cmp(pa,pb)); self.emit(enc::cset(pd,10)); self.commit_write(*d);
                }

                // ── Type conversion ──────────────────────────────────

                LIROp::ZeroExt { dst, src, from_bits } => {
                    let ps = self.r_read(*src); let pd = self.r_write(*dst);
                    let insn = match from_bits {
                        8  => enc::uxtb(pd, ps),
                        16 => enc::uxth(pd, ps),
                        32 => enc::uxtw(pd, ps),
                        _  => enc::mov_reg(pd, ps),
                    };
                    self.emit(insn); self.commit_write(*dst);
                }
                LIROp::SignExt { dst, src, from_bits } => {
                    let ps = self.r_read(*src); let pd = self.r_write(*dst);
                    let insn = match from_bits {
                        8  => enc::sxtb(pd, ps),
                        16 => enc::sxth(pd, ps),
                        32 => enc::sxtw(pd, ps),
                        _  => enc::mov_reg(pd, ps),
                    };
                    self.emit(insn); self.commit_write(*dst);
                }

                // ── Memory (64-bit) ──────────────────────────────────

                LIROp::Load { dst, base, offset } => {
                    let b = self.r_read(*base); let d = self.r_write(*dst);
                    self.emit_load64(d, b, *offset); self.commit_write(*dst);
                }
                LIROp::Store { src, base, offset } => {
                    let b = self.r_read(*base); let s = self.r_read(*src);
                    self.emit_store64(s, b, *offset);
                }

                // ── Memory (sized) ───────────────────────────────────

                LIROp::LoadSize { dst, base, offset, size } => {
                    let b = self.r_read(*base); let d = self.r_write(*dst);
                    self.emit_load_sized(d, b, *offset, *size); self.commit_write(*dst);
                }
                LIROp::StoreSize { src, base, offset, size } => {
                    let b = self.r_read(*base); let s = self.r_read(*src);
                    self.emit_store_sized(s, b, *offset, *size);
                }

                // ── Atomics ──────────────────────────────────────────

                LIROp::AtomicLoad { dst, ptr } => {
                    let pp = self.r_read(*ptr); let pd = self.r_write(*dst);
                    self.emit(enc::ldar(pd, pp)); self.commit_write(*dst);
                }
                LIROp::AtomicStore { src, ptr } => {
                    let pp = self.r_read(*ptr); let ps = self.r_read(*src);
                    self.emit(enc::stlr(ps, pp));
                }
                LIROp::AtomicXchg { dst, src, ptr } => {
                    self.emit_atomic_xchg(*dst, *src, *ptr);
                }
                LIROp::AtomicFetchAdd { dst, delta, ptr } => {
                    self.emit_atomic_fetch_binop(*dst, *delta, *ptr, true);
                }
                LIROp::AtomicFetchSub { dst, delta, ptr } => {
                    self.emit_atomic_fetch_binop(*dst, *delta, *ptr, false);
                }
                LIROp::AtomicCas { old, new, ptr, ok } => {
                    self.emit_atomic_cas(*old, *new, *ptr, *ok);
                }

                // ── Passthrough ──────────────────────────────────────

                LIROp::Asm { lines } => {
                    for line in lines {
                        match assemble_line(line.trim()) {
                            Some(insn) => self.emit(insn),
                            None => self.emit(enc::brk(0xFFFF)),
                        }
                    }
                }
            }
        }
    }

    // ── Memory helpers ───────────────────────────────────────────────

    fn emit_load64(&mut self, rd: u8, rn: u8, offset: i32) {
        if offset >= 0 && offset % 8 == 0 && (offset / 8) < 4096 {
            self.emit(enc::ldr_imm(rd, rn, (offset / 8) as u16));
        } else if (-256..=255).contains(&offset) {
            self.emit(enc::ldur(rd, rn, offset as i16));
        } else {
            self.emit_many(enc::mov_imm64(SCRATCH1, offset as u64));
            self.emit(enc::add(SCRATCH1, rn, SCRATCH1));
            self.emit(enc::ldr_imm(rd, SCRATCH1, 0));
        }
    }

    fn emit_store64(&mut self, rs: u8, rn: u8, offset: i32) {
        if offset >= 0 && offset % 8 == 0 && (offset / 8) < 4096 {
            self.emit(enc::str_imm(rs, rn, (offset / 8) as u16));
        } else if (-256..=255).contains(&offset) {
            self.emit(enc::stur(rs, rn, offset as i16));
        } else {
            self.emit_many(enc::mov_imm64(SCRATCH1, offset as u64));
            self.emit(enc::add(SCRATCH1, rn, SCRATCH1));
            self.emit(enc::str_imm(rs, SCRATCH1, 0));
        }
    }

    fn emit_load_sized(&mut self, rd: u8, rn: u8, offset: i32, size: u8) {
        // For sized loads we require non-negative, aligned offset in range.
        // Fall back to 64-bit load for out-of-range (conservative).
        let in_range = |scale: i32| -> bool {
            offset >= 0 && offset % scale == 0 && (offset / scale) < 4096
        };
        match size {
            1 if in_range(1) => self.emit(enc::ldrb_imm(rd, rn, offset as u16)),
            2 if in_range(2) => self.emit(enc::ldrh_imm(rd, rn, (offset/2) as u16)),
            4 if in_range(4) => self.emit(enc::ldrw_imm(rd, rn, (offset/4) as u16)),
            8 => self.emit_load64(rd, rn, offset),
            _ => {
                // Offset out of immediate range — add offset into scratch first
                self.emit_many(enc::mov_imm64(SCRATCH1, offset as u64));
                self.emit(enc::add(SCRATCH1, rn, SCRATCH1));
                match size {
                    1 => self.emit(enc::ldrb_imm(rd, SCRATCH1, 0)),
                    2 => self.emit(enc::ldrh_imm(rd, SCRATCH1, 0)),
                    4 => self.emit(enc::ldrw_imm(rd, SCRATCH1, 0)),
                    _ => self.emit(enc::ldr_imm(rd, SCRATCH1, 0)),
                }
            }
        }
    }

    fn emit_store_sized(&mut self, rs: u8, rn: u8, offset: i32, size: u8) {
        let in_range = |scale: i32| -> bool {
            offset >= 0 && offset % scale == 0 && (offset / scale) < 4096
        };
        match size {
            1 if in_range(1) => self.emit(enc::strb_imm(rs, rn, offset as u16)),
            2 if in_range(2) => self.emit(enc::strh_imm(rs, rn, (offset/2) as u16)),
            4 if in_range(4) => self.emit(enc::strw_imm(rs, rn, (offset/4) as u16)),
            8 => self.emit_store64(rs, rn, offset),
            _ => {
                self.emit_many(enc::mov_imm64(SCRATCH1, offset as u64));
                self.emit(enc::add(SCRATCH1, rn, SCRATCH1));
                match size {
                    1 => self.emit(enc::strb_imm(rs, SCRATCH1, 0)),
                    2 => self.emit(enc::strh_imm(rs, SCRATCH1, 0)),
                    4 => self.emit(enc::strw_imm(rs, SCRATCH1, 0)),
                    _ => self.emit(enc::str_imm(rs, SCRATCH1, 0)),
                }
            }
        }
    }

    // ── Atomic helpers ───────────────────────────────────────────────

    // LDAXR / STLXR exchange loop: dst = *ptr; *ptr = src  (returns old)
    fn emit_atomic_xchg(&mut self, dst: Reg, src: Reg, ptr: Reg) {
        let pp = self.r_read(ptr);
        let ps = self.r_read(src);
        let pd = self.r_write(dst);
        // scratch1 = status
        let retry_off = self.offset();
        self.emit(enc::ldaxr(pd, pp));
        self.emit(enc::stlxr(SCRATCH1, ps, pp));
        // CBNZ scratch1, retry (B19 patch)
        let patch_off = self.offset();
        self.patches.push((patch_off, format!("__axchg_{patch_off}"), PatchKind::B19));
        self.emit(0xB5000000 | (SCRATCH1 as u32));
        // Define retry label at ldaxr
        self.labels.insert(format!("__axchg_{patch_off}"), retry_off);
        self.commit_write(dst);
    }

    // AtomicFetchAdd/Sub: dst = *ptr; *ptr = dst ± delta
    fn emit_atomic_fetch_binop(&mut self, dst: Reg, delta: Reg, ptr: Reg, is_add: bool) {
        let pp  = self.r_read(ptr);
        let pdelta = self.r_read(delta);
        let pd  = self.r_write(dst);
        let retry_off = self.offset();
        self.emit(enc::ldaxr(pd, pp));
        if is_add { self.emit(enc::add(SCRATCH1, pd, pdelta)); }
        else       { self.emit(enc::sub(SCRATCH1, pd, pdelta)); }
        self.emit(enc::stlxr(SCRATCH0, SCRATCH1, pp));
        let patch_off = self.offset();
        self.patches.push((patch_off, format!("__afop_{patch_off}"), PatchKind::B19));
        self.emit(0xB5000000 | (SCRATCH0 as u32));
        self.labels.insert(format!("__afop_{patch_off}"), retry_off);
        self.commit_write(dst);
    }

    // AtomicCAS: if *ptr == old { *ptr = new; ok=1 } else { ok=0 }
    fn emit_atomic_cas(&mut self, old: Reg, new: Reg, ptr: Reg, ok: Reg) {
        let pp  = self.r_read(ptr);
        let pold = self.r_read(old);
        let pnew = self.r_read(new);
        let pok  = self.r_write(ok);

        let retry_off = self.offset();
        self.emit(enc::ldaxr(SCRATCH1, pp));
        self.emit(enc::cmp(SCRATCH1, pold));
        // B.NE fail
        let bne_off = self.offset();
        self.patches.push((bne_off, format!("__cas_fail_{bne_off}"), PatchKind::B19));
        // BCond NE=1 (uses B19 encoding with condition)
        self.emit(0x54000001u32); // B.NE #0 placeholder (will be patched below manually)

        self.emit(enc::stlxr(SCRATCH0, pnew, pp));
        // CBNZ SCRATCH0, retry
        let retry_patch = self.offset();
        self.patches.push((retry_patch, format!("__cas_retry_{retry_patch}"), PatchKind::B19));
        self.emit(0xB5000000 | (SCRATCH0 as u32));
        self.labels.insert(format!("__cas_retry_{retry_patch}"), retry_off);

        // ok = 1
        self.emit_many(enc::mov_imm64(pok, 1));
        // B done
        let bdone_off = self.offset();
        self.patches.push((bdone_off, format!("__cas_done_{bdone_off}"), PatchKind::B26));
        self.emit(0x14000000);

        // fail: ok = 0
        let fail_off = self.offset();
        self.labels.insert(format!("__cas_fail_{bne_off}"), fail_off);
        self.emit_many(enc::mov_imm64(pok, 0));

        // done:
        let done_off = self.offset();
        self.labels.insert(format!("__cas_done_{bdone_off}"), done_off);

        // Fix up the B.NE placeholder manually (B.cond uses 19-bit offset in bits[23:5])
        {
            let delta = (fail_off as i64 - bne_off as i64) / 4;
            let patched = 0x54000001u32 | (((delta as u32) & 0x7FFFF) << 5);
            self.code[bne_off..bne_off+4].copy_from_slice(&patched.to_le_bytes());
        }
        // Remove the B.NE entry from patches (already patched above)
        self.patches.retain(|(off,_,_)| *off != bne_off);

        self.commit_write(ok);
    }
}

fn assemble_line(line: &str) -> Option<u32> {
    let line = line.to_lowercase();
    let line = line.trim();
    if line == "ret" { return Some(enc::ret()); }
    if line == "nop" { return Some(enc::nop()); }
    if let Some(rest) = line.strip_prefix("svc #") {
        return u16::from_str_radix(rest.trim().trim_start_matches("0x"), 16)
            .ok().or_else(|| rest.trim().parse().ok())
            .map(enc::svc);
    }
    if let Some(rest) = line.strip_prefix("svc 0x") {
        let imm = u16::from_str_radix(rest.trim(), 16).ok()?;
        return Some(enc::svc(imm));
    }
    if let Some(rest) = line.strip_prefix("brk #") {
        return rest.trim().parse().ok().map(enc::brk);
    }
    None
}
