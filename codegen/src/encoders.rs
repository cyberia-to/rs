//! Pure ARM64 instruction encoders (copied from trident/src/compile/arm64_encoders.rs).
//!
//! Every function returns a u32 (or Vec<u32>) little-endian machine word.
//! No state, no allocations.

#[inline] pub fn add(rd: u8, rn: u8, rm: u8) -> u32 {
    0x8B000000 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn sub(rd: u8, rn: u8, rm: u8) -> u32 {
    0xCB000000 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn subs(rd: u8, rn: u8, rm: u8) -> u32 {
    0xEB000000 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn mul(rd: u8, rn: u8, rm: u8) -> u32 {
    0x9B007C00 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn udiv(rd: u8, rn: u8, rm: u8) -> u32 {
    0x9AC00800 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn msub(rd: u8, rn: u8, rm: u8, ra: u8) -> u32 {
    0x9B008000 | ((rm as u32) << 16) | ((ra as u32) << 10) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn cmp(rn: u8, rm: u8) -> u32 { subs(31, rn, rm) }
#[inline] pub fn neg(rd: u8, rm: u8) -> u32 { sub(rd, 31, rm) }
#[inline] pub fn add_imm(rd: u8, rn: u8, imm12: u16) -> u32 {
    0x91000000 | ((imm12 as u32) << 10) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn and(rd: u8, rn: u8, rm: u8) -> u32 {
    0x8A000000 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn orr(rd: u8, rn: u8, rm: u8) -> u32 {
    0xAA000000 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn eor(rd: u8, rn: u8, rm: u8) -> u32 {
    0xCA000000 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn mvn(rd: u8, rm: u8) -> u32 {
    0xAA2003E0 | ((rm as u32) << 16) | (rd as u32)
}
#[inline] pub fn clz(rd: u8, rn: u8) -> u32 {
    0xDAC01000 | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn lsl_reg(rd: u8, rn: u8, rm: u8) -> u32 {
    0x9AC02000 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn lsr_reg(rd: u8, rn: u8, rm: u8) -> u32 {
    0x9AC02400 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn lsr_imm(rd: u8, rn: u8, shift: u8) -> u32 {
    0xD3400000 | ((shift as u32) << 16) | (63 << 10) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn mov_reg(rd: u8, rm: u8) -> u32 {
    0xAA0003E0 | ((rm as u32) << 16) | (rd as u32)
}
#[inline] pub fn movz(rd: u8, imm16: u16) -> u32 {
    0xD2800000 | ((imm16 as u32) << 5) | (rd as u32)
}
#[inline] pub fn movk(rd: u8, imm16: u16, shift: u8) -> u32 {
    let hw = (shift / 16) as u32;
    0xF2800000 | (hw << 21) | ((imm16 as u32) << 5) | (rd as u32)
}
pub fn mov_imm64(rd: u8, val: u64) -> Vec<u32> {
    let mut v = Vec::with_capacity(4);
    v.push(movz(rd, val as u16));
    if val > 0xFFFF       { v.push(movk(rd, (val >> 16) as u16, 16)); }
    if val > 0xFFFF_FFFF  { v.push(movk(rd, (val >> 32) as u16, 32)); }
    if val > 0xFFFF_FFFF_FFFF { v.push(movk(rd, (val >> 48) as u16, 48)); }
    v
}
#[inline] pub fn cset(rd: u8, cond: u8) -> u32 {
    let inv = cond ^ 1;
    0x9A9F07E0 | ((inv as u32) << 12) | (rd as u32)
}
#[inline] pub fn ldr_imm(rd: u8, rn: u8, imm12: u16) -> u32 {
    0xF9400000 | ((imm12 as u32) << 10) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn str_imm(rs: u8, rn: u8, imm12: u16) -> u32 {
    0xF9000000 | ((imm12 as u32) << 10) | ((rn as u32) << 5) | (rs as u32)
}
#[inline] pub fn ldur(rd: u8, rn: u8, simm9: i16) -> u32 {
    let imm9 = (simm9 as u32) & 0x1FF;
    0xF8400000 | (imm9 << 12) | ((rn as u32) << 5) | (rd as u32)
}
#[inline] pub fn stur(rs: u8, rn: u8, simm9: i16) -> u32 {
    let imm9 = (simm9 as u32) & 0x1FF;
    0xF8000000 | (imm9 << 12) | ((rn as u32) << 5) | (rs as u32)
}
#[inline] pub fn stp_pre(rn: u8, rm: u8, rd: u8, simm7: i8) -> u32 {
    let imm7 = (simm7 as u32) & 0x7F;
    0xA9800000 | (imm7 << 15) | ((rm as u32) << 10) | ((rd as u32) << 5) | (rn as u32)
}
#[inline] pub fn ldp_post(rn: u8, rm: u8, rd: u8, simm7: i8) -> u32 {
    let imm7 = (simm7 as u32) & 0x7F;
    0xA8C00000 | (imm7 << 15) | ((rm as u32) << 10) | ((rd as u32) << 5) | (rn as u32)
}
#[inline] pub fn adrp(rd: u8, imm21: i32) -> u32 {
    let immlo = (imm21 as u32) & 0x3;
    let immhi = ((imm21 as u32) >> 2) & 0x7FFFF;
    0x90000000 | (immlo << 29) | (immhi << 5) | (rd as u32)
}
#[inline] pub fn bl_placeholder() -> u32 { 0x94000000 }
#[inline] pub fn blr(rn: u8) -> u32 { 0xD63F0000 | ((rn as u32) << 5) }
#[inline] pub fn ret() -> u32 { 0xD65F03C0 }
#[inline] pub fn svc(imm16: u16) -> u32 { 0xD4000001 | ((imm16 as u32) << 5) }
#[inline] pub fn brk(imm16: u16) -> u32 { 0xD4200000 | ((imm16 as u32) << 5) }
#[inline] pub fn nop() -> u32 { 0xD503201F }
// Signed divide
#[inline] pub fn sdiv(rd: u8, rn: u8, rm: u8) -> u32 {
    0x9AC00C00 | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}
// Arithmetic shift right (signed)
#[inline] pub fn asr_reg(rd: u8, rn: u8, rm: u8) -> u32 {
    0x9AC02800 | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}
// SP-relative subtract (allocate stack frame)
#[inline] pub fn sub_imm(rd: u8, rn: u8, imm12: u16) -> u32 {
    0xD1000000 | ((imm12 as u32) << 10) | ((rn as u32) << 5) | rd as u32
}
// Sign extension (SBFM x64)
#[inline] pub fn sxtb(rd: u8, rn: u8) -> u32 { 0x93401C00 | ((rn as u32) << 5) | rd as u32 }
#[inline] pub fn sxth(rd: u8, rn: u8) -> u32 { 0x93403C00 | ((rn as u32) << 5) | rd as u32 }
#[inline] pub fn sxtw(rd: u8, rn: u8) -> u32 { 0x93407C00 | ((rn as u32) << 5) | rd as u32 }
// Zero extension (UBFM x64)
#[inline] pub fn uxtb(rd: u8, rn: u8) -> u32 { 0xD3401C00 | ((rn as u32) << 5) | rd as u32 }
#[inline] pub fn uxth(rd: u8, rn: u8) -> u32 { 0xD3403C00 | ((rn as u32) << 5) | rd as u32 }
#[inline] pub fn uxtw(rd: u8, rn: u8) -> u32 { 0xD3407C00 | ((rn as u32) << 5) | rd as u32 }
// Sized loads (zero-extend to 64-bit): imm12 is in units of the access size
#[inline] pub fn ldrb_imm(rt: u8, rn: u8, imm12: u16) -> u32 {
    0x39400000 | ((imm12 as u32) << 10) | ((rn as u32) << 5) | rt as u32
}
#[inline] pub fn strb_imm(rt: u8, rn: u8, imm12: u16) -> u32 {
    0x39000000 | ((imm12 as u32) << 10) | ((rn as u32) << 5) | rt as u32
}
#[inline] pub fn ldrh_imm(rt: u8, rn: u8, imm12: u16) -> u32 {
    0x79400000 | ((imm12 as u32) << 10) | ((rn as u32) << 5) | rt as u32
}
#[inline] pub fn strh_imm(rt: u8, rn: u8, imm12: u16) -> u32 {
    0x79000000 | ((imm12 as u32) << 10) | ((rn as u32) << 5) | rt as u32
}
#[inline] pub fn ldrw_imm(rt: u8, rn: u8, imm12: u16) -> u32 {  // 32-bit, zero-ext to 64
    0xB9400000 | ((imm12 as u32) << 10) | ((rn as u32) << 5) | rt as u32
}
#[inline] pub fn strw_imm(rt: u8, rn: u8, imm12: u16) -> u32 {
    0xB9000000 | ((imm12 as u32) << 10) | ((rn as u32) << 5) | rt as u32
}
// Atomic acquire-load / release-store (64-bit)
#[inline] pub fn ldar(rt: u8, rn: u8) -> u32 { 0xC8DFFC00 | ((rn as u32) << 5) | rt as u32 }
#[inline] pub fn stlr(rt: u8, rn: u8) -> u32 { 0xC89FFC00 | ((rn as u32) << 5) | rt as u32 }
// Exclusive acquire-load / exclusive release-store (64-bit)
#[inline] pub fn ldaxr(rt: u8, rn: u8) -> u32 { 0xC85FFC00 | ((rn as u32) << 5) | rt as u32 }
#[inline] pub fn stlxr(rs: u8, rt: u8, rn: u8) -> u32 {
    0xC800FC00 | ((rs as u32) << 16) | ((rn as u32) << 5) | rt as u32
}
