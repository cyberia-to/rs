//! Assemble the final Mach-O MH_EXECUTE binary from the laid-out sections.

use std::mem;
use link::sign;

use crate::dyld;
use crate::layout::{Layout, PAGE};
use crate::resolve::SymbolTable;

// ---------------------------------------------------------------------------
// Mach-O constants
// ---------------------------------------------------------------------------
const MH_MAGIC_64: u32        = 0xFEED_FACF;
const CPU_TYPE_ARM64: i32     = 0x0100_000C;
const CPU_SUBTYPE_ARM64_ALL: i32 = 0;
const MH_EXECUTE: u32         = 2;
const MH_DYLDLINK: u32        = 0x4;
const MH_PIE: u32             = 0x0020_0000;
const MH_TWOLEVEL: u32        = 0x80;
const MH_NOUNDEFS: u32        = 0x1;

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

const PROT_R: i32  = 0x01;
const PROT_RX: i32 = 0x05;
const PROT_RW: i32 = 0x03;
const VM_BASE: u64 = 0x1_0000_0000;

// ---------------------------------------------------------------------------
// On-disk structures
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
#[repr(C, packed)] struct LinkeditDataCommand { cmd: u32, cmdsize: u32, dataoff: u32, datasize: u32 }
#[repr(C, packed)] struct BuildVersionCommand { cmd: u32, cmdsize: u32, platform: u32, minos: u32, sdk: u32, ntools: u32 }
#[repr(C, packed)] struct SymtabCommand { cmd: u32, cmdsize: u32, symoff: u32, nsyms: u32, stroff: u32, strsize: u32 }
#[repr(C, packed)] struct DysymtabCommand {
    cmd: u32, cmdsize: u32,
    ilocalsym: u32, nlocalsym: u32, iextdefsym: u32, nextdefsym: u32,
    iundefsym: u32, nundefsym: u32, tocoff: u32, ntoc: u32,
    modtaboff: u32, nmodtab: u32, extrefsymoff: u32, nextrefsyms: u32,
    indirectsymoff: u32, nindirectsyms: u32, extreloff: u32, nextrel: u32,
    locreloff: u32, nlocrel: u32,
}
#[repr(C, packed)] struct EntryPointCommand { cmd: u32, cmdsize: u32, entryoff: u64, stacksize: u64 }

unsafe fn as_bytes<T>(v: &T) -> &[u8] {
    std::slice::from_raw_parts(v as *const T as *const u8, mem::size_of::<T>())
}

fn name16(s: &str) -> [u8; 16] {
    let mut buf = [0u8; 16];
    let b = s.as_bytes();
    buf[..b.len().min(16)].copy_from_slice(&b[..b.len().min(16)]);
    buf
}

fn push_u32(v: &mut Vec<u8>, x: u32) { v.extend_from_slice(&x.to_le_bytes()); }

fn align_up(x: usize, a: usize) -> usize { (x + a - 1) & !(a - 1) }

// ---------------------------------------------------------------------------
// Emitter
// ---------------------------------------------------------------------------

pub struct EmitArgs<'a> {
    pub layout: &'a Layout,
    pub syms: &'a SymbolTable,
    pub imports: &'a [String],      // dylib import names
    pub dylibs: &'a [String],       // dylib paths
    pub entry_symbol: &'a str,
}

pub fn emit(args: &EmitArgs) -> Vec<u8> {
    let layout = args.layout;
    let imports = args.imports;

    // ---- Gather segment/section info ----
    let text_secs: Vec<_> = layout.merged.iter().enumerate()
        .filter(|(_, m)| m.seg == "__TEXT")
        .collect();
    let data_secs: Vec<_> = layout.merged.iter().enumerate()
        .filter(|(_, m)| m.seg == "__DATA" || m.seg == "__DATA_CONST")
        .collect();
    let has_data = !data_secs.is_empty();

    // ---- Compute GOT file offset for dyld fixups ----
    let (got_fileoff, got_vm) = layout.got_idx.map(|gi| {
        (layout.merged[gi].file_offset, layout.merged[gi].vm_addr)
    }).unwrap_or((layout.linkedit_fileoff, 0));

    // ---- Build dyld chained fixups blob ----
    let n_data_secs = data_secs.len();
    let cf = if !imports.is_empty() {
        dyld::build(imports, got_fileoff, layout.data_fileoff, n_data_secs)
    } else {
        dyld::ChainedFixups { blob: make_empty_chained_fixups(), got_page_start: 0 }
    };

    // ---- Write GOT bind entries into DATA section ----
    let mut merged_patched: Vec<Vec<u8>> = layout.merged.iter().map(|m| m.data.clone()).collect();
    if let Some(gi) = layout.got_idx {
        for (i, _) in imports.iter().enumerate() {
            let is_last = i + 1 == imports.len();
            let entry = dyld::got_bind_entry(i, is_last);
            let off = i * 8;
            merged_patched[gi][off..off + 8].copy_from_slice(&entry.to_le_bytes());
        }
    }

    // ---- Compute LINKEDIT layout ----
    let chain_off = layout.linkedit_fileoff;
    let chain_size = cf.blob.len();
    let trie_off   = chain_off + chain_size;
    const TRIE_SIZE: usize = 4;
    const STR_SIZE: usize = 1;
    let strtab_off = trie_off + TRIE_SIZE;
    let strtab_end = strtab_off + STR_SIZE;
    let code_limit  = align_up(strtab_end, 8);

    let identifier = "pure.rs";
    let sig_off  = code_limit;
    let sig_size = sign::signature_size(code_limit, identifier);
    let linkedit_filesize = sig_off + sig_size - layout.linkedit_fileoff;
    let linkedit_vmsize   = align_up(linkedit_filesize, PAGE) as u64;

    // ---- Compute TEXT segment boundaries ----
    let text_vmaddr = VM_BASE;
    let text_fileoff = 0u64;
    let text_vmsize = if has_data {
        layout.data_fileoff as u64
    } else {
        layout.linkedit_fileoff as u64
    };
    let text_filesize = text_vmsize;

    // ---- Compute DATA segment boundaries ----
    let data_fileoff = layout.data_fileoff as u64;
    let data_vmaddr  = layout.data_vmaddr;
    let data_size = (layout.linkedit_fileoff - layout.data_fileoff) as u64;

    // ---- Count load commands ----
    const SEG: usize = 72;
    const SEC: usize = 80;
    const LDS: usize = 16;

    let n_segs = 2 + if has_data { 1 } else { 0 } + 1; // PAGEZERO + TEXT + DATA? + LINKEDIT
    let n_text_secs = text_secs.len();
    let n_data_secs = data_secs.len();

    // Fixed load commands:
    // __PAGEZERO + __TEXT + __DATA? + __LINKEDIT +
    // LC_DYLD_CHAINED_FIXUPS + LC_DYLD_EXPORTS_TRIE +
    // LC_SYMTAB + LC_DYSYMTAB + LC_LOAD_DYLINKER +
    // LC_BUILD_VERSION + LC_MAIN + LC_LOAD_DYLIB(s) + LC_CODE_SIGNATURE
    let n_dylibs = args.dylibs.len().max(1); // at least libSystem
    let dylib_sizes: Vec<usize> = args.dylibs.iter().map(|p| {
        let total = p.len() + 1; // null-terminated
        24 + align_up(total, 8)  // 24-byte header + padded name (8-byte aligned for 64-bit)
    }).collect();
    let dylib_total: usize = if args.dylibs.is_empty() {
        56 // libSystem fallback
    } else {
        dylib_sizes.iter().sum()
    };

    let ncmds = 1 // PAGEZERO
        + 1 // TEXT
        + if has_data { 1 } else { 0 }
        + 1 // LINKEDIT
        + 1 // DYLD_CHAINED_FIXUPS
        + 1 // DYLD_EXPORTS_TRIE
        + 1 // SYMTAB
        + 1 // DYSYMTAB
        + 1 // LOAD_DYLINKER
        + 1 // BUILD_VERSION
        + 1 // MAIN
        + n_dylibs // LOAD_DYLIB(s)
        + 1;// CODE_SIGNATURE

    let sizeofcmds = SEG // PAGEZERO
        + SEG + n_text_secs * SEC
        + if has_data { SEG + n_data_secs * SEC } else { 0 }
        + SEG // LINKEDIT
        + LDS + LDS + 24 + 80 + 32 + 24 + 24
        + dylib_total
        + LDS;

    let header_total = 32 + sizeofcmds;
    let first_section_off = align_up(header_total, 16);

    // ---- Compute entry offset ----
    let entry_off = if let Some(gs) = args.syms.syms.get(args.entry_symbol) {
        gs.addr - VM_BASE
    } else if let Some(gs) = args.syms.syms.get("_main") {
        gs.addr - VM_BASE
    } else {
        layout.merged[layout.text_idx].file_offset as u64
    };

    // ---- Build output ----
    let capacity = sig_off + sig_size + 64;
    let mut out: Vec<u8> = Vec::with_capacity(capacity);

    // Mach-O header
    out.extend_from_slice(unsafe { as_bytes(&MachHeader64 {
        magic: MH_MAGIC_64, cputype: CPU_TYPE_ARM64, cpusubtype: CPU_SUBTYPE_ARM64_ALL,
        filetype: MH_EXECUTE, ncmds: ncmds as u32, sizeofcmds: sizeofcmds as u32,
        flags: MH_NOUNDEFS | MH_DYLDLINK | MH_PIE | MH_TWOLEVEL, reserved: 0,
    })});

    // LC_SEGMENT_64 __PAGEZERO
    out.extend_from_slice(unsafe { as_bytes(&SegmentCommand64 {
        cmd: LC_SEGMENT_64, cmdsize: SEG as u32, segname: name16("__PAGEZERO"),
        vmaddr: 0, vmsize: VM_BASE, fileoff: 0, filesize: 0,
        maxprot: 0, initprot: 0, nsects: 0, flags: 0,
    })});

    // LC_SEGMENT_64 __TEXT
    out.extend_from_slice(unsafe { as_bytes(&SegmentCommand64 {
        cmd: LC_SEGMENT_64, cmdsize: (SEG + n_text_secs * SEC) as u32,
        segname: name16("__TEXT"),
        vmaddr: text_vmaddr, vmsize: text_vmsize,
        fileoff: text_fileoff, filesize: text_filesize,
        maxprot: PROT_RX, initprot: PROT_RX, nsects: n_text_secs as u32, flags: 0,
    })});
    for (_, ms) in &text_secs {
        out.extend_from_slice(unsafe { as_bytes(&Section64 {
            sectname: name16(&ms.name), segname: name16("__TEXT"),
            addr: ms.vm_addr, size: ms.data.len() as u64,
            offset: ms.file_offset as u32, align: ms.align,
            reloff: 0, nreloc: 0, flags: ms.flags,
            reserved1: 0, reserved2: 0, reserved3: 0,
        })});
    }

    // LC_SEGMENT_64 __DATA (if any)
    if has_data {
        out.extend_from_slice(unsafe { as_bytes(&SegmentCommand64 {
            cmd: LC_SEGMENT_64, cmdsize: (SEG + n_data_secs * SEC) as u32,
            segname: name16("__DATA"),
            vmaddr: data_vmaddr, vmsize: data_size,
            fileoff: data_fileoff, filesize: data_size,
            maxprot: PROT_RW, initprot: PROT_RW, nsects: n_data_secs as u32, flags: 0,
        })});
        for (_, ms) in &data_secs {
            out.extend_from_slice(unsafe { as_bytes(&Section64 {
                sectname: name16(&ms.name), segname: name16("__DATA"),
                addr: ms.vm_addr, size: ms.data.len() as u64,
                offset: ms.file_offset as u32, align: ms.align,
                reloff: 0, nreloc: 0, flags: ms.flags,
                reserved1: 0, reserved2: 0, reserved3: 0,
            })});
        }
    }

    // LC_SEGMENT_64 __LINKEDIT
    out.extend_from_slice(unsafe { as_bytes(&SegmentCommand64 {
        cmd: LC_SEGMENT_64, cmdsize: SEG as u32, segname: name16("__LINKEDIT"),
        vmaddr: layout.linkedit_vmaddr, vmsize: linkedit_vmsize,
        fileoff: layout.linkedit_fileoff as u64, filesize: linkedit_filesize as u64,
        maxprot: PROT_R, initprot: PROT_R, nsects: 0, flags: 0,
    })});

    // LC_DYLD_CHAINED_FIXUPS
    out.extend_from_slice(unsafe { as_bytes(&LinkeditDataCommand {
        cmd: LC_DYLD_CHAINED_FIXUPS, cmdsize: LDS as u32,
        dataoff: chain_off as u32, datasize: chain_size as u32,
    })});

    // LC_DYLD_EXPORTS_TRIE (empty)
    out.extend_from_slice(unsafe { as_bytes(&LinkeditDataCommand {
        cmd: LC_DYLD_EXPORTS_TRIE, cmdsize: LDS as u32,
        dataoff: trie_off as u32, datasize: TRIE_SIZE as u32,
    })});

    // LC_SYMTAB (empty)
    out.extend_from_slice(unsafe { as_bytes(&SymtabCommand {
        cmd: LC_SYMTAB, cmdsize: 24,
        symoff: strtab_off as u32, nsyms: 0,
        stroff: strtab_off as u32, strsize: STR_SIZE as u32,
    })});

    // LC_DYSYMTAB (all zeros)
    out.extend_from_slice(unsafe { as_bytes(&DysymtabCommand {
        cmd: LC_DYSYMTAB, cmdsize: 80,
        ilocalsym: 0, nlocalsym: 0, iextdefsym: 0, nextdefsym: 0,
        iundefsym: 0, nundefsym: 0, tocoff: 0, ntoc: 0,
        modtaboff: 0, nmodtab: 0, extrefsymoff: 0, nextrefsyms: 0,
        indirectsymoff: 0, nindirectsyms: 0, extreloff: 0, nextrel: 0,
        locreloff: 0, nlocrel: 0,
    })});

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
    })});

    // LC_MAIN
    out.extend_from_slice(unsafe { as_bytes(&EntryPointCommand {
        cmd: LC_MAIN, cmdsize: 24, entryoff: entry_off, stacksize: 0,
    })});

    // LC_LOAD_DYLIB entries
    if args.dylibs.is_empty() {
        // Default: libSystem
        push_u32(&mut out, LC_LOAD_DYLIB);
        push_u32(&mut out, 56);
        push_u32(&mut out, 24);
        push_u32(&mut out, 2);
        push_u32(&mut out, 0x0001_0000);
        push_u32(&mut out, 0x0001_0000);
        out.extend_from_slice(b"/usr/lib/libSystem.B.dylib\0");
        out.extend_from_slice(&[0u8; 5]);
    } else {
        for (i, dylib_path) in args.dylibs.iter().enumerate() {
            let bytes = dylib_path.as_bytes();
            let name_len = bytes.len() + 1;
            let padded = align_up(name_len, 8);
            let cmdsize = 24 + padded;
            push_u32(&mut out, LC_LOAD_DYLIB);
            push_u32(&mut out, cmdsize as u32);
            push_u32(&mut out, 24);      // name offset
            push_u32(&mut out, 2);       // timestamp
            push_u32(&mut out, 0x0001_0000);
            push_u32(&mut out, 0x0001_0000);
            out.extend_from_slice(bytes);
            out.push(0); // null terminator
            let pad = padded - name_len;
            out.extend_from_slice(&vec![0u8; pad]);
        }
    }

    // LC_CODE_SIGNATURE
    out.extend_from_slice(unsafe { as_bytes(&LinkeditDataCommand {
        cmd: LC_CODE_SIGNATURE, cmdsize: LDS as u32,
        dataoff: sig_off as u32, datasize: sig_size as u32,
    })});

    // Verify header size matches expected.
    debug_assert!(out.len() <= header_total,
        "header overflow: {} > {header_total}", out.len());

    // ---- Section content ----
    out.resize(first_section_off, 0);

    // Write all sections in file-offset order.
    let mut sorted_indices: Vec<usize> = (0..layout.merged.len()).collect();
    sorted_indices.sort_by_key(|&i| layout.merged[i].file_offset);

    for i in sorted_indices {
        let ms = &layout.merged[i];
        if ms.data.is_empty() { continue; }
        if ms.file_offset < out.len() {
            // Already past this offset (shouldn't happen with correct layout).
            eprintln!("warning: section {}:{} file_offset {:#x} < out.len() {:#x}",
                      ms.seg, ms.name, ms.file_offset, out.len());
            continue;
        }
        out.resize(ms.file_offset, 0);
        out.extend_from_slice(&merged_patched[i]);
    }

    // ---- LINKEDIT content ----
    out.resize(chain_off, 0);
    out.extend_from_slice(&cf.blob);
    out.resize(trie_off, 0);
    out.extend_from_slice(&[0u8; TRIE_SIZE]); // empty exports trie
    out.push(0);                               // strtab null byte
    out.resize(code_limit, 0);

    debug_assert_eq!(out.len(), code_limit,
        "code_limit mismatch: {} vs {code_limit}", out.len());

    // ---- Code signature ----
    let exec_seg_limit = if has_data { data_fileoff as u64 } else { layout.linkedit_fileoff as u64 };
    let sig = sign::make_code_signature(&out, identifier, exec_seg_limit as u64);
    assert_eq!(sig.len(), sig_size, "signature size mismatch");
    out.extend_from_slice(&sig);

    out
}

/// Minimal chained fixups blob for zero imports (3 segments, no fixup chains).
fn make_empty_chained_fixups() -> Vec<u8> {
    // Same as Phase 1's make_chained_fixups()
    let fields: [u32; 12] = [
        0, 32, 48, 48, 0, 1, 0, 0,  // header (8 u32s = 32 bytes)
        3, 0, 0, 0,                  // starts_in_image: 3 segs, all zero offsets
    ];
    let mut v = vec![0u8; 56];
    for (i, &f) in fields.iter().enumerate() {
        v[i*4..i*4+4].copy_from_slice(&f.to_le_bytes());
    }
    v
}
