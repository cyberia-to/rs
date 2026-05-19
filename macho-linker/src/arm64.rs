//! ARM64 instruction encoding helpers used by the linker.

/// Encode ADRP Rd, #page_delta (page_delta in 4KB pages, signed 21-bit).
#[inline]
pub fn adrp(rd: u8, page_delta: i32) -> u32 {
    let imm = (page_delta as u32) & 0x1F_FFFF;
    0x9000_0000u32 | ((imm & 0x3) << 29) | (((imm >> 2) & 0x7_FFFF) << 5) | (rd as u32)
}

/// Encode ADD Rd, Rn, #imm12 (64-bit, no shift).
#[inline]
pub fn add_imm12(rd: u8, rn: u8, imm12: u32) -> u32 {
    0x9100_0000u32 | ((imm12 & 0xFFF) << 10) | ((rn as u32) << 5) | (rd as u32)
}

/// Encode LDR Rt, [Rn, #byte_offset] (64-bit unsigned offset, byte_offset must be
/// divisible by 8 and fit in 12-bit scaled field).
#[inline]
pub fn ldr64_unsigned(rt: u8, rn: u8, byte_offset: u32) -> u32 {
    let imm12 = byte_offset / 8;
    0xF940_0000u32 | ((imm12 & 0xFFF) << 10) | ((rn as u32) << 5) | (rt as u32)
}

/// BR Rn — indirect branch.
#[inline]
pub fn br(rn: u8) -> u32 {
    0xD61F_0000u32 | ((rn as u32) << 5)
}

/// RET.
#[inline]
pub fn ret() -> u32 {
    0xD65F_03C0u32
}

/// Extract Rd from any instruction that stores it in bits [4:0].
#[inline]
pub fn decode_rd(insn: u32) -> u8 {
    (insn & 0x1F) as u8
}

/// Extract Rn from bits [9:5] (used by ADD/LDR).
#[inline]
pub fn decode_rn(insn: u32) -> u8 {
    ((insn >> 5) & 0x1F) as u8
}

/// Apply a 26-bit branch displacement to a BL/B instruction.
/// delta = (target - pc) / 4, signed 26-bit range.
pub fn patch_branch26(insn: u32, delta: i32) -> u32 {
    (insn & 0xFC00_0000) | ((delta as u32) & 0x03FF_FFFF)
}

/// Apply a 21-bit ADRP page displacement.
/// page_delta = (target_4k_page - pc_4k_page) in pages.
pub fn patch_adrp(insn: u32, page_delta: i32) -> u32 {
    let imm = (page_delta as u32) & 0x1F_FFFF;
    (insn & 0x9F00_001F) | ((imm & 0x3) << 29) | (((imm >> 2) & 0x7_FFFF) << 5)
}

/// Apply a 12-bit page offset to an ADD-immediate instruction.
pub fn patch_add_pageoff(insn: u32, page_offset: u32) -> u32 {
    (insn & 0xFFC0_03FF) | ((page_offset & 0xFFF) << 10)
}

/// Apply a 12-bit page offset to an LDR/STR unsigned-offset instruction.
/// The immediate is scaled by the access size (bits [31:30]).
pub fn patch_ldr_pageoff(insn: u32, page_offset: u32) -> u32 {
    let size_shift = (insn >> 30) & 0x3; // 0=8b, 1=16b, 2=32b, 3=64b
    let scaled = page_offset >> size_shift;
    (insn & 0xFFC0_03FF) | ((scaled & 0xFFF) << 10)
}

/// True if the instruction looks like an ADD immediate (bits [31:24] = 0x91).
#[inline]
pub fn is_add_imm(insn: u32) -> bool {
    (insn >> 24) == 0x91
}
