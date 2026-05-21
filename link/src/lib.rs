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

/// A static data symbol: read-only goes to __TEXT,__cstring; writable to __DATA,__bss.
pub struct StaticData { pub name: String, pub bytes: Vec<u8>, pub writable: bool }

/// A relocation from machine code into a static data symbol (ADRP + ADD pair).
pub struct DataReloc {
    pub adrp_offset: usize,
    pub add_offset: usize,
    pub symbol: String,
}

/// A relocation within a static data section: write the VM address of `fn_symbol`
/// (a code symbol) into the static named `static_name` at `byte_offset`.
/// Used to fix up vtable entries (function pointers stored in read-only data).
pub struct StaticCodeReloc {
    pub static_name: String,
    pub byte_offset: usize,
    pub fn_symbol: String,
}

/// Input to the Mach-O emitter.
pub struct EmitInput<'a> {
    pub code: &'a [u8],
    pub data: Vec<StaticData>,
    pub relocs: Vec<DataReloc>,
    pub static_relocs: Vec<StaticCodeReloc>,
    /// Map from function symbol name to byte offset within `code`.
    /// Required to resolve `static_relocs` (vtable function pointer fixups).
    pub fn_offsets: std::collections::HashMap<String, usize>,
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

/// Minimal LC_DYLD_CHAINED_FIXUPS blob with configurable segment count (3 or 4).
///
/// Always 56 bytes: 32-byte header + 4*(seg_count+1) bytes starts_in_image + padding.
/// seg_count=3: PAGEZERO+TEXT+LINKEDIT; seg_count=4: PAGEZERO+TEXT+DATA+LINKEDIT.
fn make_chained_fixups(seg_count: u32) -> [u8; 56] {
    // imports/symbols start right after the starts_in_image structure:
    // header(32) + seg_count(4) + seg_offsets(4*seg_count)
    let imports_off = 32u32 + 4 + 4 * seg_count;
    let mut v = [0u8; 56];
    // 8-word header
    v[4..8].copy_from_slice(&32u32.to_le_bytes());              // starts_offset
    v[8..12].copy_from_slice(&imports_off.to_le_bytes());       // imports_offset
    v[12..16].copy_from_slice(&imports_off.to_le_bytes());      // symbols_offset
    v[20..24].copy_from_slice(&1u32.to_le_bytes());             // imports_format = DYLD_CHAINED_IMPORT
    // starts_in_image (at byte 32)
    v[32..36].copy_from_slice(&seg_count.to_le_bytes());        // seg_count
    // seg_info_offsets all zero — no fixup chains in any segment
    v
}

// ---------------------------------------------------------------------------
// Core emitter
// ---------------------------------------------------------------------------

const PROT_RW: i32 = 0x03;
const S_ZEROFILL: u32 = 1;

/// Emit a complete Mach-O MH_EXECUTE binary with ad-hoc code signature.
///
/// Read-only statics go to __TEXT,__cstring.
/// Writable (BSS) statics go to __DATA,__bss (S_ZEROFILL, no file content).
pub fn emit_macho(input: &EmitInput) -> Vec<u8> {
    const SEG: usize = 72;   // SegmentCommand64
    const SEC: usize = 80;   // Section64
    const LDS: usize = 16;   // LinkeditDataCommand

    // Partition statics into read-only (cstring) and writable (BSS).
    // Own mutable byte copies so we can patch vtable function-pointer slots.
    let mut ro_data: Vec<(String, Vec<u8>)> = input.data.iter()
        .filter(|d| !d.writable)
        .map(|d| (d.name.clone(), d.bytes.clone()))
        .collect();
    let rw_data: Vec<&StaticData> = input.data.iter().filter(|d|  d.writable).collect();

    let cstring_size: usize = ro_data.iter().map(|(_, b)| b.len()).sum();
    let bss_size:     usize = rw_data.iter().map(|d| d.bytes.len()).sum();
    let has_data = bss_size > 0;
    let bss_vm_pages = if has_data { align_up(bss_size, MACHO_PAGE) } else { 0 };

    // Symbol-to-offset maps for relocation patching.
    let mut ro_offsets: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut rw_offsets: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    {
        let mut c = 0usize;
        for (name, bytes) in &ro_data { ro_offsets.insert(name.clone(), c); c += bytes.len(); }
    }
    {
        let mut c = 0usize;
        for s in &rw_data { rw_offsets.insert(s.name.clone(), c); c += s.bytes.len(); }
    }

    // Load-command table sizes.
    let data_lc_size = if has_data { SEG + SEC } else { 0 };
    let ncmds: u32 = if has_data { 13 } else { 12 };
    let total_lc: usize = SEG             // __PAGEZERO
        + SEG + 2 * SEC                   // __TEXT (2 sections)
        + data_lc_size                    // __DATA (optional)
        + SEG                             // __LINKEDIT
        + LDS                             // LC_DYLD_CHAINED_FIXUPS
        + LDS                             // LC_DYLD_EXPORTS_TRIE
        + 24                              // LC_SYMTAB
        + 80                              // LC_DYSYMTAB
        + 32                              // LC_LOAD_DYLINKER
        + 24                              // LC_BUILD_VERSION
        + 24                              // LC_MAIN
        + 56                              // LC_LOAD_DYLIB libSystem
        + LDS;                            // LC_CODE_SIGNATURE

    let header_total     = 32 + total_lc;
    let code_file_offset = align_up(header_total, 16);
    let code_size        = input.code.len();

    let cstring_file_offset = align_up(code_file_offset + code_size, 4);
    let content_end         = cstring_file_offset + cstring_size;
    let linkedit_off        = align_up(content_end, MACHO_PAGE);

    // VM layout.
    let code_vm_addr    = VM_BASE + code_file_offset as u64;
    let cstring_vm_addr = VM_BASE + cstring_file_offset as u64;
    let text_vmsize     = linkedit_off as u64;

    let data_vmaddr      = VM_BASE + text_vmsize;                  // right after __TEXT
    let linkedit_vmaddr  = data_vmaddr + bss_vm_pages as u64;

    // __LINKEDIT layout (unchanged by __DATA presence since BSS has no file content).
    const CHAIN_SIZE: usize = 56;
    const TRIE_SIZE: usize  = 4;
    const STR_SIZE: usize   = 1;
    let chain_off  = linkedit_off;
    let trie_off   = chain_off + CHAIN_SIZE;
    let str_off    = trie_off  + TRIE_SIZE;
    let code_limit = align_up(str_off + STR_SIZE, 8);

    let identifier        = "pure.rs";
    let sig_off           = code_limit;
    let sig_size          = sign::signature_size(code_limit, identifier);
    let linkedit_filesize = sig_off + sig_size - linkedit_off;
    let linkedit_vmsize   = align_up(linkedit_filesize, MACHO_PAGE) as u64;

    // Patch vtable function-pointer slots: write the absolute VM address of each
    // function into the corresponding ro_data (or rw_data) static byte slice.
    // This must happen before writing section bytes to the output buffer.
    for sr in &input.static_relocs {
        let fn_vm = if let Some(&fn_off) = input.fn_offsets.get(&sr.fn_symbol) {
            code_vm_addr + fn_off as u64
        } else {
            // Unresolved function symbol in vtable: emit zero (safe fallback — will crash
            // at runtime if the method is actually called, but doesn't corrupt the binary).
            0u64
        };
        // Find the static in ro_data and patch the slot.
        let mut patched = false;
        for (name, bytes) in &mut ro_data {
            if *name == sr.static_name {
                let end = sr.byte_offset + 8;
                if end <= bytes.len() {
                    bytes[sr.byte_offset..end].copy_from_slice(&fn_vm.to_le_bytes());
                }
                patched = true;
                break;
            }
        }
        if !patched {
            // The static may be writable (TLS/BSS) — we can't patch zerofill data here,
            // but vtables are always read-only, so this case should not arise in practice.
        }
    }

    // Patch ADRP+ADD relocations for both RO (cstring) and RW (BSS) symbols.
    let mut code = input.code.to_vec();
    for r in &input.relocs {
        let target = if let Some(&off) = ro_offsets.get(&r.symbol) {
            cstring_vm_addr + off as u64
        } else if let Some(&off) = rw_offsets.get(&r.symbol) {
            data_vmaddr + off as u64
        } else {
            panic!("unknown reloc symbol: {}", r.symbol);
        };
        let adrp_pc    = code_vm_addr + r.adrp_offset as u64;
        let page_delta = (((target & !0xFFF) as i64) - ((adrp_pc & !0xFFF) as i64)) >> 12;
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

    // Mach-O header.
    out.extend_from_slice(unsafe { as_bytes(&MachHeader64 {
        magic: MH_MAGIC_64, cputype: CPU_TYPE_ARM64, cpusubtype: CPU_SUBTYPE_ARM64_ALL,
        filetype: MH_EXECUTE, ncmds, sizeofcmds: total_lc as u32,
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
        vmaddr: VM_BASE, vmsize: text_vmsize, fileoff: 0, filesize: text_vmsize,
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

    // LC_SEGMENT_64 __DATA + __bss section (only when writable statics exist).
    if has_data {
        out.extend_from_slice(unsafe { as_bytes(&SegmentCommand64 {
            cmd: LC_SEGMENT_64, cmdsize: (SEG + SEC) as u32, segname: name16("__DATA"),
            vmaddr: data_vmaddr, vmsize: bss_vm_pages as u64,
            fileoff: linkedit_off as u64,  // S_ZEROFILL: no file content
            filesize: 0,
            maxprot: PROT_RW, initprot: PROT_RW, nsects: 1, flags: 0,
        }) });
        out.extend_from_slice(unsafe { as_bytes(&Section64 {
            sectname: name16("__bss"), segname: name16("__DATA"),
            addr: data_vmaddr, size: bss_size as u64,
            offset: 0,           // S_ZEROFILL: no file offset
            align: 3,            // 8-byte align
            reloff: 0, nreloc: 0,
            flags: S_ZEROFILL,
            reserved1: 0, reserved2: 0, reserved3: 0,
        }) });
    }

    // LC_SEGMENT_64 __LINKEDIT
    out.extend_from_slice(unsafe { as_bytes(&SegmentCommand64 {
        cmd: LC_SEGMENT_64, cmdsize: SEG as u32, segname: name16("__LINKEDIT"),
        vmaddr: linkedit_vmaddr, vmsize: linkedit_vmsize,
        fileoff: linkedit_off as u64, filesize: linkedit_filesize as u64,
        maxprot: PROT_R, initprot: PROT_R, nsects: 0, flags: 0,
    }) });

    // LC_DYLD_CHAINED_FIXUPS
    let seg_count = if has_data { 4u32 } else { 3u32 };
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

    // LC_LOAD_DYLINKER
    push_u32(&mut out, LC_LOAD_DYLINKER);
    push_u32(&mut out, 32);
    push_u32(&mut out, 12);
    out.extend_from_slice(b"/usr/lib/dyld\0");
    out.extend_from_slice(&[0u8; 6]);

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

    // LC_LOAD_DYLIB /usr/lib/libSystem.B.dylib (56 bytes)
    push_u32(&mut out, LC_LOAD_DYLIB);
    push_u32(&mut out, 56);
    push_u32(&mut out, 24);
    push_u32(&mut out, 2);
    push_u32(&mut out, 0x0001_0000);
    push_u32(&mut out, 0x0001_0000);
    out.extend_from_slice(b"/usr/lib/libSystem.B.dylib\0");
    out.extend_from_slice(&[0u8; 5]);

    // LC_CODE_SIGNATURE
    out.extend_from_slice(unsafe { as_bytes(&LinkeditDataCommand {
        cmd: LC_CODE_SIGNATURE, cmdsize: LDS as u32,
        dataoff: sig_off as u32, datasize: sig_size as u32,
    }) });

    debug_assert_eq!(out.len(), header_total, "LC size mismatch");

    // File content.
    out.resize(code_file_offset, 0);
    out.extend_from_slice(&code);
    out.resize(cstring_file_offset, 0);
    for (_, bytes) in &ro_data { out.extend_from_slice(bytes); }  // read-only data only
    out.resize(linkedit_off, 0);
    // BSS statics have no file content — dyld zero-initialises them at load time.

    // __LINKEDIT.
    out.extend_from_slice(&make_chained_fixups(seg_count));
    out.extend_from_slice(&[0u8; TRIE_SIZE]);
    out.push(0x00);
    out.resize(code_limit, 0);

    debug_assert_eq!(out.len(), code_limit);

    // Code signature covers [0..code_limit]; execSegLimit = __TEXT filesize = linkedit_off.
    let sig = sign::make_code_signature(&out, identifier, linkedit_off as u64);
    assert_eq!(sig.len(), sig_size, "signature size mismatch — layout bug");
    out.extend_from_slice(&sig);

    out
}

// ---------------------------------------------------------------------------
// Relocatable object (.o) emitter — MH_OBJECT
// ---------------------------------------------------------------------------

const MH_OBJECT: u32 = 0x1;

// ARM64 relocation types (arm64/reloc.h)
const ARM64_RELOC_BRANCH26:  u32 = 2;
const ARM64_RELOC_PAGE21:    u32 = 3;
const ARM64_RELOC_PAGEOFF12: u32 = 4;

// nlist64 n_type values
const N_EXT:  u8 = 0x01;   // external symbol bit
const N_SECT: u8 = 0x0E;   // symbol defined in a section (local)
// global defined = N_SECT | N_EXT = 0x0F
// undefined extern = N_EXT alone = 0x01

/// A defined function symbol within __text.
pub struct FnSymbol {
    pub name: String,
    pub offset: usize,   // byte offset within __text
    pub is_global: bool,
}

/// A BL instruction relocation targeting a named symbol.
pub struct CallReloc2 {
    pub offset: usize,   // byte offset of BL instruction in __text
    pub symbol: String,  // target symbol name
}

/// Input for the MH_OBJECT emitter.
pub struct RelocatableInput<'a> {
    pub code:        &'a [u8],
    pub ro_data:     Vec<StaticData>,   // string literals → __TEXT,__cstring
    pub rw_data:     Vec<StaticData>,   // writable statics → __DATA,__bss (zerofill)
    pub call_relocs: Vec<CallReloc2>,   // BL relocations for function calls
    pub data_relocs: Vec<DataReloc>,    // ADRP+ADD pairs for data references
    pub fn_syms:     Vec<FnSymbol>,     // defined function symbols
}

// Mach-O relocation entry: 8 bytes.
#[repr(C, packed)]
struct RelocationInfo {
    r_address: i32,
    r_info:    u32,   // packed: symbolnum[23:0] | pcrel[24] | length[26:25] | extern[27] | type[31:28]
}

// nlist64 symbol table entry: 16 bytes.
#[repr(C, packed)]
struct Nlist64 {
    n_strx:  u32,
    n_type:  u8,
    n_sect:  u8,
    n_desc:  u16,
    n_value: u64,
}

fn reloc_info(symbolnum: u32, pcrel: bool, r_extern: bool, r_type: u32) -> u32 {
    // r_length=2 (4-byte instructions) for all ARM64 instruction relocs.
    let r_length: u32 = 2;
    (symbolnum & 0x00FF_FFFF)
        | ((pcrel as u32) << 24)
        | (r_length << 25)
        | ((r_extern as u32) << 27)
        | (r_type << 28)
}

/// Emit a Mach-O MH_OBJECT relocatable file for ARM64.
///
/// Layout:
///   [header 32b] [load commands] [__text data] [__cstring data]
///   [__text relocs] [__cstring relocs=0] [nlist64 symtab] [strtab]
///
/// BSS (__DATA,__bss) is S_ZEROFILL — no file content, fileoff=0.
pub fn emit_object(input: &RelocatableInput) -> Vec<u8> {
    const SEG: usize = 72;
    const SEC: usize = 80;
    const RELOC_SIZE: usize = 8;
    const NLIST_SIZE: usize = 16;

    // Determine which optional sections exist.
    let has_cstring = !input.ro_data.is_empty();
    let has_bss     = !input.rw_data.is_empty();

    // Section index map (1-based for n_sect):
    //   1 = __text
    //   2 = __cstring  (if present, else absent)
    //   3 = __bss      (if present, else 2 if no cstring)
    let text_sect_idx: u8   = 1;
    let cstring_sect_idx: u8 = if has_cstring { 2 } else { 0 };
    let bss_sect_idx: u8 = match (has_cstring, has_bss) {
        (_, false) => 0,
        (true, true) => 3,
        (false, true) => 2,
    };

    let nsects: u32 = 1
        + if has_cstring { 1 } else { 0 }
        + if has_bss     { 1 } else { 0 };

    // Accumulate cstring offsets (offset within __cstring section).
    let mut cstring_offsets: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    {
        let mut pos = 0usize;
        for s in &input.ro_data {
            cstring_offsets.insert(s.name.clone(), pos);
            pos += s.bytes.len();
        }
    }
    let cstring_size: usize = input.ro_data.iter().map(|s| s.bytes.len()).sum();

    // BSS offsets (section-relative).
    let mut bss_offsets: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    {
        let mut pos = 0usize;
        for s in &input.rw_data {
            bss_offsets.insert(s.name.clone(), pos);
            pos += s.bytes.len();
        }
    }
    let bss_size: usize = input.rw_data.iter().map(|s| s.bytes.len()).sum();

    // -----------------------------------------------------------------------
    // Build the symbol table in required order:
    //   [locals] [global defs] [undefined externs]
    // -----------------------------------------------------------------------

    // Collect names of all defined symbols in this TU.
    let mut defined_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for fs in &input.fn_syms {
        defined_names.insert(fs.name.clone());
    }
    for s in &input.ro_data {
        defined_names.insert(s.name.clone());
    }
    for s in &input.rw_data {
        defined_names.insert(s.name.clone());
    }

    // Collect undefined externals (symbols referenced but not defined here).
    let mut undef_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for cr in &input.call_relocs {
        if !defined_names.contains(&cr.symbol) {
            undef_names.insert(cr.symbol.clone());
        }
    }
    for dr in &input.data_relocs {
        if !defined_names.contains(&dr.symbol) {
            undef_names.insert(dr.symbol.clone());
        }
    }

    // Symbol ordering: local fn_syms, global fn_syms, local data syms, global data syms, undefs.
    // In practice fn_syms has is_global flag; data symbols default to local.
    struct SymEntry {
        name:    String,
        n_type:  u8,
        n_sect:  u8,
        n_value: u64,
    }

    let mut sym_entries: Vec<SymEntry> = Vec::new();

    // Local fn symbols.
    for fs in &input.fn_syms {
        if !fs.is_global {
            sym_entries.push(SymEntry {
                name:    fs.name.clone(),
                n_type:  N_SECT,
                n_sect:  text_sect_idx,
                n_value: fs.offset as u64,
            });
        }
    }
    // Local data symbols (ro_data).
    if has_cstring {
        for s in &input.ro_data {
            sym_entries.push(SymEntry {
                name:    s.name.clone(),
                n_type:  N_SECT,
                n_sect:  cstring_sect_idx,
                n_value: *cstring_offsets.get(&s.name).unwrap() as u64,
            });
        }
    }
    // Local data symbols (rw_data).
    if has_bss {
        for s in &input.rw_data {
            sym_entries.push(SymEntry {
                name:    s.name.clone(),
                n_type:  N_SECT,
                n_sect:  bss_sect_idx,
                n_value: *bss_offsets.get(&s.name).unwrap() as u64,
            });
        }
    }

    let n_local = sym_entries.len() as u32;

    // Global fn symbols.
    for fs in &input.fn_syms {
        if fs.is_global {
            sym_entries.push(SymEntry {
                name:    fs.name.clone(),
                n_type:  N_SECT | N_EXT,
                n_sect:  text_sect_idx,
                n_value: fs.offset as u64,
            });
        }
    }

    let n_global = sym_entries.len() as u32 - n_local;

    // Undefined externals.
    for name in &undef_names {
        sym_entries.push(SymEntry {
            name:    name.clone(),
            n_type:  N_EXT,
            n_sect:  0,
            n_value: 0,
        });
    }

    let n_undef = sym_entries.len() as u32 - n_local - n_global;
    let nsyms   = sym_entries.len() as u32;

    // Build string table: \0 + name\0 for each entry.
    let mut strtab: Vec<u8> = vec![0u8];
    let mut sym_strx: Vec<u32> = Vec::with_capacity(sym_entries.len());
    for se in &sym_entries {
        sym_strx.push(strtab.len() as u32);
        strtab.extend_from_slice(se.name.as_bytes());
        strtab.push(0u8);
    }
    let strtab_size = strtab.len() as u32;

    // Build symbol → table index map for relocation entries.
    let sym_index: std::collections::HashMap<String, u32> = sym_entries
        .iter()
        .enumerate()
        .map(|(i, se)| (se.name.clone(), i as u32))
        .collect();

    // -----------------------------------------------------------------------
    // Relocation entries for __text.
    // -----------------------------------------------------------------------
    let mut text_relocs: Vec<RelocationInfo> = Vec::new();
    for cr in &input.call_relocs {
        let sym_idx = *sym_index.get(&cr.symbol)
            .unwrap_or_else(|| panic!("call reloc symbol not in table: {}", cr.symbol));
        text_relocs.push(RelocationInfo {
            r_address: cr.offset as i32,
            r_info: reloc_info(sym_idx, true, true, ARM64_RELOC_BRANCH26),
        });
    }
    for dr in &input.data_relocs {
        let sym_idx = *sym_index.get(&dr.symbol)
            .unwrap_or_else(|| panic!("data reloc symbol not in table: {}", dr.symbol));
        // ADRP: PAGE21, pcrel=1, extern=1
        text_relocs.push(RelocationInfo {
            r_address: dr.adrp_offset as i32,
            r_info: reloc_info(sym_idx, true, true, ARM64_RELOC_PAGE21),
        });
        // ADD: PAGEOFF12, pcrel=0, extern=1
        text_relocs.push(RelocationInfo {
            r_address: dr.add_offset as i32,
            r_info: reloc_info(sym_idx, false, true, ARM64_RELOC_PAGEOFF12),
        });
    }
    let n_text_relocs = text_relocs.len() as u32;

    // -----------------------------------------------------------------------
    // File layout calculation.
    // -----------------------------------------------------------------------
    // Load commands size.
    let lc_seg_size: usize = SEG + nsects as usize * SEC;
    let lc_symtab_size: usize = 24;
    let total_lc: usize = lc_seg_size + lc_symtab_size;

    let header_size: usize = 32;
    let header_total: usize = header_size + total_lc;

    // Section data immediately after load commands (no padding needed for ARM64 .o).
    let text_off:    usize = header_total;
    let text_size:   usize = input.code.len();
    let cstring_off: usize = align_up(text_off + text_size, 4);
    let data_end:    usize = cstring_off + cstring_size;

    // Relocation entries follow section data (8-byte aligned).
    let text_reloc_off: usize = align_up(data_end, 4);
    let text_reloc_end: usize = text_reloc_off + n_text_relocs as usize * RELOC_SIZE;

    // Symbol table and string table follow relocs (4-byte aligned).
    let symtab_off: usize = align_up(text_reloc_end, 4);
    let strtab_off: usize = symtab_off + nsyms as usize * NLIST_SIZE;

    // -----------------------------------------------------------------------
    // Output assembly.
    // -----------------------------------------------------------------------
    let total_size = strtab_off + strtab_size as usize;
    let mut out: Vec<u8> = Vec::with_capacity(total_size + 64);

    // Mach-O header.
    out.extend_from_slice(unsafe { as_bytes(&MachHeader64 {
        magic: MH_MAGIC_64, cputype: CPU_TYPE_ARM64, cpusubtype: CPU_SUBTYPE_ARM64_ALL,
        filetype: MH_OBJECT, ncmds: 2,
        sizeofcmds: total_lc as u32,
        flags: 0, reserved: 0,
    }) });

    // LC_SEGMENT_64 (single segment, segname="" for MH_OBJECT).
    // vmaddr=0, vmsize=covers all sections, fileoff=0.
    let seg_vmsize: u64 = (cstring_off + cstring_size + bss_size) as u64;
    out.extend_from_slice(unsafe { as_bytes(&SegmentCommand64 {
        cmd: LC_SEGMENT_64, cmdsize: lc_seg_size as u32,
        segname: [0u8; 16],
        vmaddr: 0, vmsize: seg_vmsize,
        fileoff: 0, filesize: (data_end) as u64,
        maxprot: PROT_RX, initprot: PROT_RX,
        nsects, flags: 0,
    }) });

    // Section: __TEXT,__text
    out.extend_from_slice(unsafe { as_bytes(&Section64 {
        sectname: name16("__text"), segname: name16("__TEXT"),
        addr: 0, size: text_size as u64,
        offset: text_off as u32, align: 2,
        reloff: if n_text_relocs > 0 { text_reloc_off as u32 } else { 0 },
        nreloc: n_text_relocs,
        flags: 0x8000_0400,
        reserved1: 0, reserved2: 0, reserved3: 0,
    }) });

    // Section: __TEXT,__cstring (optional)
    if has_cstring {
        out.extend_from_slice(unsafe { as_bytes(&Section64 {
            sectname: name16("__cstring"), segname: name16("__TEXT"),
            addr: cstring_off as u64, size: cstring_size as u64,
            offset: cstring_off as u32, align: 0,
            reloff: 0, nreloc: 0,
            flags: S_CSTRING_LITERALS,
            reserved1: 0, reserved2: 0, reserved3: 0,
        }) });
    }

    // Section: __DATA,__bss (optional, S_ZEROFILL, no file content)
    if has_bss {
        // For MH_OBJECT the bss addr follows cstring in the flat address space.
        let bss_addr = (cstring_off + cstring_size) as u64;
        out.extend_from_slice(unsafe { as_bytes(&Section64 {
            sectname: name16("__bss"), segname: name16("__DATA"),
            addr: bss_addr, size: bss_size as u64,
            offset: 0, align: 3,
            reloff: 0, nreloc: 0,
            flags: S_ZEROFILL,
            reserved1: 0, reserved2: 0, reserved3: 0,
        }) });
    }

    // LC_SYMTAB
    out.extend_from_slice(unsafe { as_bytes(&SymtabCommand {
        cmd: LC_SYMTAB, cmdsize: 24,
        symoff: symtab_off as u32, nsyms,
        stroff: strtab_off as u32, strsize: strtab_size,
    }) });

    debug_assert_eq!(out.len(), header_total, "load command size mismatch");

    // __text bytes.
    out.resize(text_off, 0);
    out.extend_from_slice(input.code);

    // __cstring bytes.
    out.resize(cstring_off, 0);
    for s in &input.ro_data {
        out.extend_from_slice(&s.bytes);
    }

    // Relocation entries.
    out.resize(text_reloc_off, 0);
    for r in &text_relocs {
        out.extend_from_slice(unsafe { as_bytes(r) });
    }

    // nlist64 symbol table.
    out.resize(symtab_off, 0);
    for (i, se) in sym_entries.iter().enumerate() {
        out.extend_from_slice(unsafe { as_bytes(&Nlist64 {
            n_strx:  sym_strx[i],
            n_type:  se.n_type,
            n_sect:  se.n_sect,
            n_desc:  0,
            n_value: se.n_value,
        }) });
    }

    // String table.
    debug_assert_eq!(out.len(), strtab_off);
    out.extend_from_slice(&strtab);

    debug_assert_eq!(out.len(), total_size);

    // LC_SYMTAB counts for external symbol categorisation (not emitted separately,
    // but record for callers that inspect ilocalsym/iextdefsym/iundefsym).
    let _ = (n_local, n_global, n_undef);

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
            static_relocs: vec![],
            fn_offsets: std::collections::HashMap::new(),
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
            data: vec![StaticData { name: "__msg".to_string(), bytes: msg.to_vec(), writable: false }],
            relocs: vec![],
            static_relocs: vec![],
            fn_offsets: std::collections::HashMap::new(),
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
            static_relocs: vec![],
            fn_offsets: std::collections::HashMap::new(),
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
            data: vec![StaticData { name: "__test".to_string(), bytes: msg.to_vec(), writable: false }],
            relocs: vec![DataReloc { adrp_offset: 0, add_offset: 4, symbol: "__test".to_string() }],
            static_relocs: vec![],
            fn_offsets: std::collections::HashMap::new(),
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
                writable: false,
            }],
            relocs: vec![DataReloc { adrp_offset: 0, add_offset: 4, symbol: "__msg".to_string() }],
            static_relocs: vec![],
            fn_offsets: std::collections::HashMap::new(),
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

        let binary = emit_macho(&EmitInput { code: &code, data: vec![], relocs: vec![], static_relocs: vec![], fn_offsets: std::collections::HashMap::new(), entry_offset: 0 });

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
