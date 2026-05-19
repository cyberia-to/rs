//! Pure-Rust Mach-O dynamic executable emitter for arm64 macOS.
//!
//! Produces a dynamically-linked MH_EXECUTE binary (LC_MAIN + dyld + libSystem).
//! Segments: __PAGEZERO, __TEXT (__text + __cstring), __LINKEDIT.

pub mod sign;

use std::mem;

// ---------------------------------------------------------------------------
// Mach-O constants
// ---------------------------------------------------------------------------

const MH_MAGIC_64: u32    = 0xFEED_FACF;
const CPU_TYPE_ARM64: i32 = 0x0100_000C;
const CPU_SUBTYPE_ARM64_ALL: i32 = 0;
const MH_EXECUTE: u32     = 2;
const MH_DYLDLINK: u32    = 0x4;
const MH_PIE: u32         = 0x0020_0000;
const MH_TWOLEVEL: u32    = 0x80;

const LC_SEGMENT_64: u32          = 0x19;
const LC_BUILD_VERSION: u32       = 0x32;
const LC_CODE_SIGNATURE: u32      = 0x1D;
const LC_LOAD_DYLINKER: u32       = 0xE;
const LC_MAIN: u32                = 0x8000_0028;
const LC_LOAD_DYLIB: u32          = 0xC;
const LC_SYMTAB: u32              = 0x2;
const LC_DYSYMTAB: u32            = 0xB;
const LC_DYLD_CHAINED_FIXUPS: u32 = 0x8000_0034;
const LC_DYLD_EXPORTS_TRIE: u32   = 0x8000_0033;

const MH_NOUNDEFS: u32 = 0x1;
const PROT_R: i32  = 0x01;
const PROT_RX: i32 = 0x05;
const VM_BASE: u64 = 0x1_0000_0000;
const S_CSTRING_LITERALS: u32 = 0x02;
/// arm64 macOS VM page size — segments must be aligned to this boundary.
const MACHO_PAGE: usize = 16384;

// ---------------------------------------------------------------------------
// Public API types
// ---------------------------------------------------------------------------

/// A static data symbol to embed in __TEXT,__cstring.
pub struct StaticData { pub name: String, pub bytes: Vec<u8> }

/// A relocation from machine code into a static data symbol (ADRP + ADD pair).
pub struct DataReloc {
    pub adrp_offset: usize,
    pub add_offset: usize,
    pub symbol: String,
}

/// Input to the Mach-O emitter.
pub struct EmitInput<'a> {
    pub code: &'a [u8],
    pub data: Vec<StaticData>,
    pub relocs: Vec<DataReloc>,
    pub entry_offset: usize,
}

// ---------------------------------------------------------------------------
// Mach-O on-disk structures (packed, little-endian)
// ---------------------------------------------------------------------------

#[repr(C, packed)] struct MachHeader64 {
    magic: u32, cputype: i32, cpusubtype: i32, filetype: u32,
    ncmds: u32, sizeofcmds: u32, flags: u32, reserved: u32,
}
#[repr(C, packed)] struct SegmentCommand64 {
    cmd: u32, cmdsize: u32, segname: [u8; 16],
    vmaddr: u64, vmsize: u64, fileoff: u64, filesize: u64,
    maxprot: i32, initprot: i32, nsects: u32, flags: u32,
}
#[repr(C, packed)] struct Section64 {
    sectname: [u8; 16], segname: [u8; 16], addr: u64, size: u64,
    offset: u32, align: u32, reloff: u32, nreloc: u32, flags: u32,
    reserved1: u32, reserved2: u32, reserved3: u32,
}
#[repr(C, packed)] struct LinkeditDataCommand {
    cmd: u32, cmdsize: u32, dataoff: u32, datasize: u32,
}
#[repr(C, packed)] struct BuildVersionCommand {
    cmd: u32, cmdsize: u32, platform: u32, minos: u32, sdk: u32, ntools: u32,
}
#[repr(C, packed)] struct SymtabCommand {
    cmd: u32, cmdsize: u32, symoff: u32, nsyms: u32, stroff: u32, strsize: u32,
}
#[repr(C, packed)] struct DysymtabCommand {
    cmd: u32, cmdsize: u32,
    ilocalsym: u32, nlocalsym: u32, iextdefsym: u32, nextdefsym: u32,
    iundefsym: u32, nundefsym: u32, tocoff: u32, ntoc: u32,
    modtaboff: u32, nmodtab: u32, extrefsymoff: u32, nextrefsyms: u32,
    indirectsymoff: u32, nindirectsyms: u32, extreloff: u32, nextrel: u32,
    locreloff: u32, nlocrel: u32,
}
#[repr(C, packed)] struct EntryPointCommand {
    cmd: u32, cmdsize: u32, entryoff: u64, stacksize: u64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn name16(s: &str) -> [u8; 16] {
    let mut buf = [0u8; 16];
    let b = s.as_bytes();
    let n = b.len().min(16);
    buf[..n].copy_from_slice(&b[..n]);
    buf
}

/// # Safety: T must be #[repr(C, packed)] with no padding and no invalid bit patterns.
unsafe fn as_bytes<T: Sized>(val: &T) -> &[u8] {
    std::slice::from_raw_parts(val as *const T as *const u8, mem::size_of::<T>())
}

fn align_up(offset: usize, align: usize) -> usize {
    (offset + align - 1) & !(align - 1)
}

#[inline] fn push_u32(v: &mut Vec<u8>, x: u32) { v.extend_from_slice(&x.to_le_bytes()); }

// ---------------------------------------------------------------------------
// ADRP / ADD instruction encoding helpers
// ---------------------------------------------------------------------------

fn encode_adrp(rd: u32, page_offset: i32) -> u32 {
    let imm = (page_offset as u32) & 0x1F_FFFF;
    0x9000_0000u32 | ((imm & 0x3) << 29) | (((imm >> 2) & 0x7_FFFF) << 5) | (rd & 0x1F)
}

fn encode_add_imm(rd: u32, rn: u32, imm12: u32) -> u32 {
    0x9100_0000u32 | ((imm12 & 0xFFF) << 10) | ((rn & 0x1F) << 5) | (rd & 0x1F)
}

fn decode_rd(insn: u32)     -> u32 { insn & 0x1F }
fn decode_rn_add(insn: u32) -> u32 { (insn >> 5) & 0x1F }

// ---------------------------------------------------------------------------
// __LINKEDIT data builders
// ---------------------------------------------------------------------------

/// Minimal LC_DYLD_CHAINED_FIXUPS blob: 3 segments, no fixups, no imports.
///
/// dyld_chained_fixups_header is 32 bytes (8 × u32, including a reserved field).
/// starts_offset must be 32 (past the full 32-byte header), not 28.
fn make_chained_fixups() -> [u8; 56] {
    // header (32 bytes = 8 u32s)
    // + dyld_chained_starts_in_image (16 bytes: seg_count=3, three zero offsets)
    // + 8 bytes zero padding (imports + symbols area, both empty)
    let fields: [u32; 12] = [
        0,   // fixups_version
        32,  // starts_offset (starts_in_image right after 32-byte header)
        48,  // imports_offset (32+16, no imports)
        48,  // symbols_offset (same, no symbols)
        0,   // imports_count
        1,   // imports_format = DYLD_CHAINED_IMPORT
        0,   // symbols_format
        0,   // reserved
        // dyld_chained_starts_in_image
        3,   // seg_count
        0,   // seg_info_offset[0] __PAGEZERO
        0,   // seg_info_offset[1] __TEXT
        0,   // seg_info_offset[2] __LINKEDIT
    ];
    let mut v = [0u8; 56]; // last 8 bytes stay zero (padding)
    for (i, &f) in fields.iter().enumerate() {
        v[i*4..i*4+4].copy_from_slice(&f.to_le_bytes());
    }
    v
}

// ---------------------------------------------------------------------------
// Core emitter
// ---------------------------------------------------------------------------

/// Emit a complete Mach-O MH_EXECUTE binary with ad-hoc code signature.
pub fn emit_macho(input: &EmitInput) -> Vec<u8> {
    // Fixed load command sizes
    const SEG: usize = 72;  // SegmentCommand64
    const SEC: usize = 80;  // Section64
    const LDS: usize = 16;  // LinkeditDataCommand

    let total_lc: usize = SEG            // __PAGEZERO
        + SEG + 2 * SEC                  // __TEXT (2 sections)
        + SEG                            // __LINKEDIT
        + LDS                            // LC_DYLD_CHAINED_FIXUPS
        + LDS                            // LC_DYLD_EXPORTS_TRIE
        + 24                             // LC_SYMTAB
        + 80                             // LC_DYSYMTAB
        + 32                             // LC_LOAD_DYLINKER
        + 24                             // LC_BUILD_VERSION
        + 24                             // LC_MAIN
        + 56                             // LC_LOAD_DYLIB libSystem
        + LDS;                           // LC_CODE_SIGNATURE
    // = 72+232+72+16+16+24+80+32+24+24+56+16 = 664

    let header_total     = 32 + total_lc;          // 696
    let code_file_offset = align_up(header_total, 16); // 704

    let code_size           = input.code.len();
    let cstring_file_offset = align_up(code_file_offset + code_size, 4);
    let cstring_size: usize = input.data.iter().map(|d| d.bytes.len()).sum();
    let content_end         = cstring_file_offset + cstring_size;
    let linkedit_off        = align_up(content_end, MACHO_PAGE); // __LINKEDIT on 16KB page boundary

    // __LINKEDIT layout
    const CHAIN_SIZE: usize = 56;
    const TRIE_SIZE: usize  = 4;
    const STR_SIZE: usize   = 1;
    let chain_off   = linkedit_off;
    let trie_off    = chain_off + CHAIN_SIZE;
    let str_off     = trie_off  + TRIE_SIZE;
    let code_limit  = align_up(str_off + STR_SIZE, 8); // boundary of hashed content

    let identifier  = "pure.rs";
    let sig_off     = code_limit;
    let sig_size    = sign::signature_size(code_limit, identifier);
    let linkedit_filesize = sig_off + sig_size - linkedit_off;
    let linkedit_vmsize   = align_up(linkedit_filesize, MACHO_PAGE) as u64;
    let linkedit_vmaddr   = VM_BASE + linkedit_off as u64;

    // VM addresses
    let code_vm_addr    = VM_BASE + code_file_offset as u64;
    let cstring_vm_addr = VM_BASE + cstring_file_offset as u64;
    let text_filesize   = linkedit_off as u64;
    let text_vmsize     = linkedit_off as u64;

    // Symbol→offset map within cstring section
    let mut sym_offsets = std::collections::HashMap::new();
    {
        let mut cursor = 0usize;
        for s in &input.data { sym_offsets.insert(s.name.clone(), cursor); cursor += s.bytes.len(); }
    }

    // Patch ADRP+ADD relocations
    let mut code = input.code.to_vec();
    for r in &input.relocs {
        let sym_off = *sym_offsets.get(&r.symbol)
            .unwrap_or_else(|| panic!("unknown reloc symbol: {}", r.symbol));
        let target      = cstring_vm_addr + sym_off as u64;
        let adrp_pc     = code_vm_addr + r.adrp_offset as u64;
        let page_delta  = (((target & !0xFFF) as i64) - ((adrp_pc & !0xFFF) as i64)) >> 12;
        assert!(page_delta >= -(1 << 20) && page_delta < (1 << 20), "ADRP out of range");
        let adrp_raw = u32::from_le_bytes(code[r.adrp_offset..][..4].try_into().unwrap());
        let add_raw  = u32::from_le_bytes(code[r.add_offset ..][..4].try_into().unwrap());
        code[r.adrp_offset..][..4].copy_from_slice(
            &encode_adrp(decode_rd(adrp_raw), page_delta as i32).to_le_bytes());
        code[r.add_offset ..][..4].copy_from_slice(
            &encode_add_imm(decode_rd(add_raw), decode_rn_add(add_raw),
                            (target & 0xFFF) as u32).to_le_bytes());
    }

    let mut out: Vec<u8> = Vec::with_capacity(sig_off + sig_size + 64);

    // Mach-O header (12 load commands)
    out.extend_from_slice(unsafe { as_bytes(&MachHeader64 {
        magic: MH_MAGIC_64, cputype: CPU_TYPE_ARM64, cpusubtype: CPU_SUBTYPE_ARM64_ALL,
        filetype: MH_EXECUTE, ncmds: 12, sizeofcmds: total_lc as u32,
        flags: MH_NOUNDEFS | MH_DYLDLINK | MH_PIE | MH_TWOLEVEL, reserved: 0,
    }) });

    // LC_SEGMENT_64 __PAGEZERO
    out.extend_from_slice(unsafe { as_bytes(&SegmentCommand64 {
        cmd: LC_SEGMENT_64, cmdsize: SEG as u32, segname: name16("__PAGEZERO"),
        vmaddr: 0, vmsize: VM_BASE, fileoff: 0, filesize: 0,
        maxprot: 0, initprot: 0, nsects: 0, flags: 0,
    }) });

    // LC_SEGMENT_64 __TEXT + 2 sections
    out.extend_from_slice(unsafe { as_bytes(&SegmentCommand64 {
        cmd: LC_SEGMENT_64, cmdsize: (SEG + 2*SEC) as u32, segname: name16("__TEXT"),
        vmaddr: VM_BASE, vmsize: text_vmsize, fileoff: 0, filesize: text_filesize,
        maxprot: PROT_RX, initprot: PROT_RX, nsects: 2, flags: 0,
    }) });
    out.extend_from_slice(unsafe { as_bytes(&Section64 {
        sectname: name16("__text"), segname: name16("__TEXT"),
        addr: code_vm_addr, size: code_size as u64,
        offset: code_file_offset as u32, align: 2,
        reloff: 0, nreloc: 0, flags: 0x8000_0400,
        reserved1: 0, reserved2: 0, reserved3: 0,
    }) });
    out.extend_from_slice(unsafe { as_bytes(&Section64 {
        sectname: name16("__cstring"), segname: name16("__TEXT"),
        addr: cstring_vm_addr, size: cstring_size as u64,
        offset: if cstring_size > 0 { cstring_file_offset as u32 } else { 0 },
        align: 0, reloff: 0, nreloc: 0, flags: S_CSTRING_LITERALS,
        reserved1: 0, reserved2: 0, reserved3: 0,
    }) });

    // LC_SEGMENT_64 __LINKEDIT
    out.extend_from_slice(unsafe { as_bytes(&SegmentCommand64 {
        cmd: LC_SEGMENT_64, cmdsize: SEG as u32, segname: name16("__LINKEDIT"),
        vmaddr: linkedit_vmaddr, vmsize: linkedit_vmsize,
        fileoff: linkedit_off as u64, filesize: linkedit_filesize as u64,
        maxprot: PROT_R, initprot: PROT_R, nsects: 0, flags: 0,
    }) });

    // LC_DYLD_CHAINED_FIXUPS
    out.extend_from_slice(unsafe { as_bytes(&LinkeditDataCommand {
        cmd: LC_DYLD_CHAINED_FIXUPS, cmdsize: LDS as u32,
        dataoff: chain_off as u32, datasize: CHAIN_SIZE as u32,
    }) });

    // LC_DYLD_EXPORTS_TRIE
    out.extend_from_slice(unsafe { as_bytes(&LinkeditDataCommand {
        cmd: LC_DYLD_EXPORTS_TRIE, cmdsize: LDS as u32,
        dataoff: trie_off as u32, datasize: TRIE_SIZE as u32,
    }) });

    // LC_SYMTAB (0 symbols, 1-byte strtab)
    out.extend_from_slice(unsafe { as_bytes(&SymtabCommand {
        cmd: LC_SYMTAB, cmdsize: 24,
        symoff: str_off as u32, nsyms: 0,
        stroff: str_off as u32, strsize: STR_SIZE as u32,
    }) });

    // LC_DYSYMTAB
    out.extend_from_slice(unsafe { as_bytes(&DysymtabCommand {
        cmd: LC_DYSYMTAB, cmdsize: 80,
        ilocalsym: 0, nlocalsym: 0, iextdefsym: 0, nextdefsym: 0,
        iundefsym: 0, nundefsym: 0, tocoff: 0, ntoc: 0,
        modtaboff: 0, nmodtab: 0, extrefsymoff: 0, nextrefsyms: 0,
        indirectsymoff: 0, nindirectsyms: 0, extreloff: 0, nextrel: 0,
        locreloff: 0, nlocrel: 0,
    }) });

    // LC_LOAD_DYLINKER (32 bytes: 12-byte header + "/usr/lib/dyld\0" + 6 pad)
    push_u32(&mut out, LC_LOAD_DYLINKER);
    push_u32(&mut out, 32);
    push_u32(&mut out, 12); // name at offset 12
    out.extend_from_slice(b"/usr/lib/dyld\0");  // 14 bytes
    out.extend_from_slice(&[0u8; 6]);            // pad to 32

    // LC_BUILD_VERSION (macOS 15.0)
    out.extend_from_slice(unsafe { as_bytes(&BuildVersionCommand {
        cmd: LC_BUILD_VERSION, cmdsize: 24, platform: 1,
        minos: 0x000F_0000, sdk: 0x000F_0000, ntools: 0,
    }) });

    // LC_MAIN
    out.extend_from_slice(unsafe { as_bytes(&EntryPointCommand {
        cmd: LC_MAIN, cmdsize: 24,
        entryoff: (code_file_offset + input.entry_offset) as u64,
        stacksize: 0,
    }) });

    // LC_LOAD_DYLIB /usr/lib/libSystem.B.dylib
    // (56 bytes: 24-byte header + "/usr/lib/libSystem.B.dylib\0" + 5 pad)
    push_u32(&mut out, LC_LOAD_DYLIB);
    push_u32(&mut out, 56);
    push_u32(&mut out, 24);           // name at offset 24
    push_u32(&mut out, 2);            // timestamp
    push_u32(&mut out, 0x0001_0000);  // current_version  1.0.0
    push_u32(&mut out, 0x0001_0000);  // compatibility_version 1.0.0
    out.extend_from_slice(b"/usr/lib/libSystem.B.dylib\0"); // 27 bytes
    out.extend_from_slice(&[0u8; 5]);                        // pad to 56

    // LC_CODE_SIGNATURE
    out.extend_from_slice(unsafe { as_bytes(&LinkeditDataCommand {
        cmd: LC_CODE_SIGNATURE, cmdsize: LDS as u32,
        dataoff: sig_off as u32, datasize: sig_size as u32,
    }) });

    debug_assert_eq!(out.len(), header_total);

    // Content
    out.resize(code_file_offset, 0);
    out.extend_from_slice(&code);
    out.resize(cstring_file_offset, 0);
    for s in &input.data { out.extend_from_slice(&s.bytes); }
    out.resize(linkedit_off, 0);

    // __LINKEDIT data
    out.extend_from_slice(&make_chained_fixups()); // 56 bytes
    out.extend_from_slice(&[0u8; TRIE_SIZE]);      // exports trie (empty root)
    out.push(0x00);                                 // strtab null byte
    out.resize(code_limit, 0);                      // align to 8

    debug_assert_eq!(out.len(), code_limit);

    // Code signature (hashes bytes [0..code_limit]; execSegLimit = __TEXT.filesize = linkedit_off)
    let sig = sign::make_code_signature(&out, identifier, linkedit_off as u64);
    assert_eq!(sig.len(), sig_size, "signature size mismatch — layout bug");
    out.extend_from_slice(&sig);

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_minimal_hello_world() {
        let input = EmitInput {
            code: &[0xC0, 0x03, 0x5F, 0xD6], // RET placeholder
            data: vec![],
            relocs: vec![],
            entry_offset: 0,
        };
        let b = emit_macho(&input);
        assert_eq!(&b[0..4], &[0xCF, 0xFA, 0xED, 0xFE]);
        assert_eq!(u32::from_le_bytes(b[12..16].try_into().unwrap()), 2); // MH_EXECUTE
    }

    #[test]
    fn emit_with_cstring_data() {
        let msg = b"Hello, world!\n\0";
        let input = EmitInput {
            code: &[0xC0, 0x03, 0x5F, 0xD6],
            data: vec![StaticData { name: "__msg".to_string(), bytes: msg.to_vec() }],
            relocs: vec![],
            entry_offset: 0,
        };
        let b = emit_macho(&input);
        assert_eq!(&b[0..4], &[0xCF, 0xFA, 0xED, 0xFE]);
        assert!(b.windows(msg.len()).any(|w| w == msg.as_ref()), "cstring not found");
    }

    #[test]
    fn lc_main_entryoff_correct() {
        let input = EmitInput {
            code: &[0xC0, 0x03, 0x5F, 0xD6],
            data: vec![],
            relocs: vec![],
            entry_offset: 0,
        };
        let b = emit_macho(&input);
        let mut off = 32usize;
        let mut found: Option<u64> = None;
        while off + 8 <= b.len() {
            let cmd     = u32::from_le_bytes(b[off..off+4].try_into().unwrap());
            let cmdsize = u32::from_le_bytes(b[off+4..off+8].try_into().unwrap()) as usize;
            if cmd == LC_MAIN {
                // EntryPointCommand: cmd(4)+cmdsize(4)+entryoff(8)
                found = Some(u64::from_le_bytes(b[off+8..off+16].try_into().unwrap()));
                break;
            }
            if cmdsize == 0 { break; }
            off += cmdsize;
        }
        let entryoff = found.expect("LC_MAIN not found");
        // code starts at code_file_offset = align_up(32+664, 16) = 704
        assert!(entryoff >= 704, "entryoff {entryoff} < code_file_offset 704");
    }

    #[test]
    fn adrp_add_patching() {
        let adrp_placeholder = encode_adrp(0, 0);
        let add_placeholder  = encode_add_imm(0, 0, 0);
        let mut code = vec![0u8; 8];
        code[0..4].copy_from_slice(&adrp_placeholder.to_le_bytes());
        code[4..8].copy_from_slice(&add_placeholder.to_le_bytes());
        let msg = b"test\0";
        let input = EmitInput {
            code: &code,
            data: vec![StaticData { name: "__test".to_string(), bytes: msg.to_vec() }],
            relocs: vec![DataReloc { adrp_offset: 0, add_offset: 4, symbol: "__test".to_string() }],
            entry_offset: 0,
        };
        let b = emit_macho(&input);
        assert_eq!(&b[0..4], &[0xCF, 0xFA, 0xED, 0xFE]);
        assert!(b.windows(msg.len()).any(|w| w == msg.as_ref()), "cstring not found");
    }

    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn hello_world_executes() {
        use std::os::unix::fs::PermissionsExt;

        // ARM64: ADRP x1 (placeholder) + ADD x1 (placeholder)
        //        MOVZ x0,#1  MOVZ x2,#14  MOVZ x16,#4  SVC #0x80   ; write(1,msg,14)
        //        MOVZ x0,#0  MOVZ x16,#1  SVC #0x80                 ; exit(0)
        let mut code: Vec<u8> = Vec::new();
        code.extend_from_slice(&0x90000001u32.to_le_bytes()); // ADRP x1, 0
        code.extend_from_slice(&0x91000021u32.to_le_bytes()); // ADD  x1, x1, #0
        code.extend_from_slice(&0xD2800020u32.to_le_bytes()); // MOVZ x0, #1
        code.extend_from_slice(&0xD28001C2u32.to_le_bytes()); // MOVZ x2, #14
        code.extend_from_slice(&0xD2800090u32.to_le_bytes()); // MOVZ x16, #4
        code.extend_from_slice(&0xD4001001u32.to_le_bytes()); // SVC  #0x80
        code.extend_from_slice(&0xD2800000u32.to_le_bytes()); // MOVZ x0, #0
        code.extend_from_slice(&0xD2800030u32.to_le_bytes()); // MOVZ x16, #1
        code.extend_from_slice(&0xD4001001u32.to_le_bytes()); // SVC  #0x80

        let input = EmitInput {
            code: &code,
            data: vec![StaticData {
                name: "__msg".to_string(),
                bytes: b"Hello, world!\n".to_vec(),
            }],
            relocs: vec![DataReloc { adrp_offset: 0, add_offset: 4, symbol: "__msg".to_string() }],
            entry_offset: 0,
        };

        let binary = emit_macho(&input);
        assert_eq!(&binary[0..4], &[0xCF, 0xFA, 0xED, 0xFE], "bad magic");

        let path = std::env::temp_dir().join("pure_rs_hello_world_test");
        eprintln!("binary: {:?}, {} bytes", path, binary.len());
        std::fs::write(&path, &binary).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let out = std::process::Command::new(&path)
            .output()
            .unwrap_or_else(|e| panic!("exec failed: {e}"));
        let _ = std::fs::remove_file(&path);

        assert_eq!(out.stdout, b"Hello, world!\n",
            "stdout={:?} status={:?} stderr={:?}", out.stdout, out.status, out.stderr);
        assert_eq!(out.status.code(), Some(0));
    }

    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn exit_code_propagates() {
        use std::os::unix::fs::PermissionsExt;

        // MOVZ x0, #42  MOVZ x16, #1  SVC #0x80  => exit(42)
        let mut code: Vec<u8> = Vec::new();
        code.extend_from_slice(&0xD2800540u32.to_le_bytes()); // MOVZ x0, #42
        code.extend_from_slice(&0xD2800030u32.to_le_bytes()); // MOVZ x16, #1
        code.extend_from_slice(&0xD4001001u32.to_le_bytes()); // SVC #0x80

        let binary = emit_macho(&EmitInput { code: &code, data: vec![], relocs: vec![], entry_offset: 0 });

        let path = std::env::temp_dir().join("pure_rs_exit42");
        std::fs::write(&path, &binary).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let status = std::process::Command::new(&path)
            .status()
            .unwrap_or_else(|e| panic!("exec failed: {e}"));
        let _ = std::fs::remove_file(&path);

        assert_eq!(status.code(), Some(42), "expected exit 42, got {status:?}");
    }
}
